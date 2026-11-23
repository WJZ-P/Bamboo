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

pub mod armadillo;
pub mod attributes;
pub mod avoid;
pub mod axolotl;
pub mod bat;
pub mod bee;
pub mod bee_goals;
pub mod bogged;
pub mod brain;
pub mod bucketable;
pub mod camel;
pub mod cat;
pub mod chicken;
pub mod control;
pub mod conversion;
pub mod creeper;
pub mod crossbow;
pub mod damage;
pub mod dolphin;
pub mod dolphin_goals;
pub mod drowned;
pub mod effects;
pub(crate) mod enderman;
pub mod endermite;
pub mod equine;
pub mod equipment;
pub mod fish;
pub mod fox;
pub mod fox_goals;
pub mod frog;
pub mod goals;
pub mod goat;
mod golem_goals;
pub mod infested;
pub mod interact;
pub mod iron_golem;
pub mod living;
pub mod mooshroom;
pub mod motion;
pub mod natural;
pub mod natural_gen;
pub mod navigation;
pub mod nbt;
pub mod neutral;
pub mod ocelot;
pub mod panda;
pub mod parrot;
pub mod path;
pub mod patrol;
pub mod phantom;
pub mod phantom_spawner;
mod pig;
pub mod pillager;
pub mod polar_bear;
pub mod rabbit;
pub mod raider;
pub mod sheep;
pub mod silverfish;
pub mod skeleton;
mod skeleton_drops;
pub mod slime;
pub mod smooth_swim;
pub mod snow_golem;
pub mod spawn;
pub mod spider;
pub mod squid;
pub mod stray;
pub mod tadpole;
pub mod tame;
pub(crate) mod timing;
pub mod turtle;
mod water;
pub mod wild_spawn;
pub mod witch;
pub mod wolf;
pub mod wolf_goals;
mod zombie_spawn;

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

/// 载具收到 `START_RIDING_JUMP`：官方 `ServerGamePacketListenerImpl.handlePlayerCommand` 里
/// 那一支，`canJump()` 就是「上了鞍」。
///
/// The vehicle got `START_RIDING_JUMP`: the branch of vanilla
/// `ServerGamePacketListenerImpl.handlePlayerCommand`, where `canJump()` means
/// "saddled".
pub fn start_riding_jump(entity: &mut super::Entity, mob: &mut Mob, scale: i32) -> bool {
    if scale <= 0
        || mob
            .equipment
            .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Saddle)
            .is_empty()
    {
        return false;
    }
    if mob.mob_type.is_equine() {
        self::equine::handle_start_jump(entity, mob);
    }
    true
}

/// 生物种类。牛走 `Animal` 一支，僵尸和尸壳走 `Monster` → `Zombie` 一支。
///
/// Mob kinds. Cows use the `Animal` branch; zombies and husks the
/// `Monster` → `Zombie` branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobType {
    #[default]
    Cow,
    Mooshroom,
    Rabbit,
    Fox,
    Wolf,
    Pig,
    Sheep,
    Chicken,
    PolarBear,
    Panda,
    Ocelot,
    Cat,
    Parrot,
    Zombie,
    Husk,
    Skeleton,
    Stray,
    Bogged,
    Slime,
    Drowned,
    Witch,
    Pillager,
    Creeper,
    Spider,
    CaveSpider,
    Enderman,
    Squid,
    GlowSquid,
    Cod,
    Salmon,
    TropicalFish,
    Pufferfish,
    Turtle,
    Dolphin,
    Axolotl,
    Horse,
    Donkey,
    Mule,
    Llama,
    Armadillo,
    Goat,
    Frog,
    Tadpole,
    Bat,
    Phantom,
    Camel,
    Bee,
    IronGolem,
    SnowGolem,
    Silverfish,
    Endermite,
}

impl MobType {
    /// 全部已建模的种类，按名字或注册表 ID 查找时遍历它。
    ///
    /// Every modelled kind; lookups by name or registry id walk this list.
    pub const ALL: [Self; 51] = [
        Self::Cow,
        Self::Mooshroom,
        Self::Rabbit,
        Self::Fox,
        Self::Wolf,
        Self::Pig,
        Self::Sheep,
        Self::Chicken,
        Self::PolarBear,
        Self::Panda,
        Self::Ocelot,
        Self::Cat,
        Self::Parrot,
        Self::Horse,
        Self::Donkey,
        Self::Mule,
        Self::Llama,
        Self::Camel,
        Self::Zombie,
        Self::Husk,
        Self::Skeleton,
        Self::Stray,
        Self::Bogged,
        Self::Slime,
        Self::Drowned,
        Self::Witch,
        Self::Pillager,
        Self::Creeper,
        Self::Spider,
        Self::CaveSpider,
        Self::Enderman,
        Self::Squid,
        Self::GlowSquid,
        Self::Cod,
        Self::Salmon,
        Self::TropicalFish,
        Self::Pufferfish,
        Self::Turtle,
        Self::Dolphin,
        Self::Axolotl,
        Self::Armadillo,
        Self::Goat,
        Self::Frog,
        Self::Tadpole,
        Self::Bat,
        Self::Phantom,
        Self::Bee,
        Self::IronGolem,
        Self::SnowGolem,
        Self::Silverfish,
        Self::Endermite,
    ];

    /// 注册表 ID。
    pub const fn entity_type(self) -> i32 {
        use mc_bamboo_data::registries::entity_type as ids;
        match self {
            Self::Cow => ids::COW,
            Self::Mooshroom => ids::MOOSHROOM,
            Self::Rabbit => ids::RABBIT,
            Self::Fox => ids::FOX,
            Self::Wolf => ids::WOLF,
            Self::Pig => ids::PIG,
            Self::Sheep => ids::SHEEP,
            Self::Chicken => ids::CHICKEN,
            Self::PolarBear => ids::POLAR_BEAR,
            Self::Panda => ids::PANDA,
            Self::Ocelot => ids::OCELOT,
            Self::Cat => ids::CAT,
            Self::Parrot => ids::PARROT,
            Self::Zombie => ids::ZOMBIE,
            Self::Husk => ids::HUSK,
            Self::Skeleton => ids::SKELETON,
            Self::Stray => ids::STRAY,
            Self::Bogged => ids::BOGGED,
            Self::Slime => ids::SLIME,
            Self::Drowned => ids::DROWNED,
            Self::Witch => ids::WITCH,
            Self::Pillager => ids::PILLAGER,
            Self::Creeper => ids::CREEPER,
            Self::Spider => ids::SPIDER,
            Self::CaveSpider => ids::CAVE_SPIDER,
            Self::Enderman => ids::ENDERMAN,
            Self::Squid => ids::SQUID,
            Self::GlowSquid => ids::GLOW_SQUID,
            Self::Cod => ids::COD,
            Self::Salmon => ids::SALMON,
            Self::TropicalFish => ids::TROPICAL_FISH,
            Self::Pufferfish => ids::PUFFERFISH,
            Self::Turtle => ids::TURTLE,
            Self::Dolphin => ids::DOLPHIN,
            Self::Axolotl => ids::AXOLOTL,
            Self::Horse => ids::HORSE,
            Self::Donkey => ids::DONKEY,
            Self::Mule => ids::MULE,
            Self::Llama => ids::LLAMA,
            Self::Armadillo => ids::ARMADILLO,
            Self::Goat => ids::GOAT,
            Self::Frog => ids::FROG,
            Self::Tadpole => ids::TADPOLE,
            Self::Bat => ids::BAT,
            Self::Phantom => ids::PHANTOM,
            Self::Camel => ids::CAMEL,
            Self::Bee => ids::BEE,
            Self::IronGolem => ids::IRON_GOLEM,
            Self::SnowGolem => ids::SNOW_GOLEM,
            Self::Silverfish => ids::SILVERFISH,
            Self::Endermite => ids::ENDERMITE,
        }
    }

    /// 存档 / 命令里的官方名字。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cow => "minecraft:cow",
            Self::Mooshroom => "minecraft:mooshroom",
            Self::Rabbit => "minecraft:rabbit",
            Self::Fox => "minecraft:fox",
            Self::Wolf => "minecraft:wolf",
            Self::Pig => "minecraft:pig",
            Self::Sheep => "minecraft:sheep",
            Self::Chicken => "minecraft:chicken",
            Self::PolarBear => "minecraft:polar_bear",
            Self::Panda => "minecraft:panda",
            Self::Ocelot => "minecraft:ocelot",
            Self::Cat => "minecraft:cat",
            Self::Parrot => "minecraft:parrot",
            Self::Zombie => "minecraft:zombie",
            Self::Husk => "minecraft:husk",
            Self::Skeleton => "minecraft:skeleton",
            Self::Stray => "minecraft:stray",
            Self::Bogged => "minecraft:bogged",
            Self::Slime => "minecraft:slime",
            Self::Drowned => "minecraft:drowned",
            Self::Witch => "minecraft:witch",
            Self::Pillager => "minecraft:pillager",
            Self::Creeper => "minecraft:creeper",
            Self::Spider => "minecraft:spider",
            Self::CaveSpider => "minecraft:cave_spider",
            Self::Enderman => "minecraft:enderman",
            Self::Squid => "minecraft:squid",
            Self::GlowSquid => "minecraft:glow_squid",
            Self::Cod => "minecraft:cod",
            Self::Salmon => "minecraft:salmon",
            Self::TropicalFish => "minecraft:tropical_fish",
            Self::Pufferfish => "minecraft:pufferfish",
            Self::Turtle => "minecraft:turtle",
            Self::Dolphin => "minecraft:dolphin",
            Self::Axolotl => "minecraft:axolotl",
            Self::Horse => "minecraft:horse",
            Self::Donkey => "minecraft:donkey",
            Self::Mule => "minecraft:mule",
            Self::Llama => "minecraft:llama",
            Self::Armadillo => "minecraft:armadillo",
            Self::Goat => "minecraft:goat",
            Self::Frog => "minecraft:frog",
            Self::Tadpole => "minecraft:tadpole",
            Self::Bat => "minecraft:bat",
            Self::Phantom => "minecraft:phantom",
            Self::Camel => "minecraft:camel",
            Self::Bee => "minecraft:bee",
            Self::IronGolem => "minecraft:iron_golem",
            Self::SnowGolem => "minecraft:snow_golem",
            Self::Silverfish => "minecraft:silverfish",
            Self::Endermite => "minecraft:endermite",
        }
    }

    pub const fn food_tag(self) -> Option<mc_bamboo_data::ItemTag> {
        use mc_bamboo_data::tags::item_tags;
        match self {
            Self::Cow => Some(item_tags::COW_FOOD),
            Self::Rabbit => Some(item_tags::RABBIT_FOOD),
            Self::Fox => Some(item_tags::FOX_FOOD),
            Self::Wolf => Some(item_tags::WOLF_FOOD),
            Self::Armadillo => Some(item_tags::ARMADILLO_FOOD),
            Self::Goat => Some(item_tags::GOAT_FOOD),
            Self::Frog | Self::Tadpole => Some(item_tags::FROG_FOOD),
            Self::Camel => Some(item_tags::CAMEL_FOOD),
            Self::Bee => Some(item_tags::BEE_FOOD),
            Self::Turtle => Some(item_tags::TURTLE_FOOD),
            Self::Axolotl => Some(item_tags::AXOLOTL_FOOD),
            Self::Ocelot => Some(item_tags::OCELOT_FOOD),
            Self::Cat => Some(item_tags::CAT_FOOD),
            _ => None,
        }
    }

    /// 这个实体类型在不在某个 `minecraft:entity_type` 标签里。
    ///
    /// Whether this entity type is in a `minecraft:entity_type` tag.
    pub fn in_entity_tag(self, name: &str) -> bool {
        let id = self.entity_type();
        mc_bamboo_data::tags::TAG_GROUPS
            .iter()
            .filter(|group| group.registry == "minecraft:entity_type")
            .flat_map(|group| group.tags.iter())
            .any(|tag| tag.name == name && tag.ids.contains(&id))
    }

    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    pub const fn is_equine(self) -> bool {
        matches!(
            self,
            Self::Horse | Self::Donkey | Self::Mule | Self::Llama | Self::Camel
        )
    }

    /// 成年和幼年的（宽，高，眼高）。
    pub fn dimensions(self, baby: bool) -> (f32, f32, f32) {
        match (self, baby) {
            // 三种骷髅的 `EntityType` 都是 `sized(0.6F, 1.99F).eyeHeight(1.74F)`。
            // All three skeletons are `sized(0.6F, 1.99F).eyeHeight(1.74F)`.
            (Self::Skeleton | Self::Stray | Self::Bogged, _) => {
                let facts = &mc_bamboo_data::entity_types::FACTS[self.entity_type() as usize];
                (
                    facts.width,
                    facts.height,
                    mc_bamboo_data::entity_types::SKELETON_EYE_HEIGHT,
                )
            }
            (Self::Creeper, _) => creeper::dimensions(),
            (Self::Enderman, _) => enderman::dimensions(),
            (Self::Spider, _) => {
                let facts = &mc_bamboo_data::entity_types::FACTS[self.entity_type() as usize];
                (
                    facts.width,
                    facts.height,
                    mc_bamboo_data::entity_types::SPIDER_EYE_HEIGHT,
                )
            }
            (Self::CaveSpider, _) => {
                let facts = &mc_bamboo_data::entity_types::FACTS[self.entity_type() as usize];
                (
                    facts.width,
                    facts.height,
                    mc_bamboo_data::entity_types::CAVE_SPIDER_EYE_HEIGHT,
                )
            }
            (Self::Rabbit, baby) => rabbit::dimensions(baby),
            (Self::Fox, baby) => fox::dimensions(baby),
            (Self::Wolf, baby) => wolf::dimensions(baby),
            // 大小 1 的尺寸；别的大小看 [`Mob::dimensions`]。
            // Size 1; other sizes go through [`Mob::dimensions`].
            (Self::Slime, _) => slime::dimensions(1),
            (Self::Horse | Self::Donkey | Self::Mule | Self::Llama, _) => {
                equine::dimensions(self, baby)
            }
            (Self::Pig, baby) => pig::dimensions(baby),
            (Self::Armadillo, baby) => armadillo::dimensions(baby),
            (Self::Goat, baby) => goat::dimensions(baby),
            (Self::Frog, baby) => frog::dimensions(baby),
            (Self::Tadpole, _) => tadpole::dimensions(),
            // 大小 0 的尺寸；别的大小看 [`Mob::dimensions`]。
            // Size 0; other sizes go through [`Mob::dimensions`].
            (Self::Phantom, _) => phantom::dimensions(0),
            // `EntityTypes.BAT`：`sized(0.5F, 0.9F).eyeHeight(0.45F)`。
            // `EntityTypes.BAT`: `sized(0.5F, 0.9F).eyeHeight(0.45F)`.
            (Self::Bat, _) => {
                let facts = &mc_bamboo_data::entity_types::FACTS[self.entity_type() as usize];
                (facts.width, facts.height, 0.45)
            }
            // 坐姿尺寸在 `Mob::dimensions` 里按 `camel.pose` 覆盖。
            // Sitting dimensions are overridden by `Mob::dimensions` from `camel.pose`.
            (Self::Camel, baby) => camel::dimensions(baby, false),
            (Self::Bee, _) => bee::dimensions(),
            (Self::IronGolem, _) => iron_golem::dimensions(),
            (Self::SnowGolem, _) => snow_golem::dimensions(),
            (Self::Sheep, baby) => sheep::dimensions(baby),
            (Self::Chicken, baby) => chicken::dimensions(baby),
            (Self::Mooshroom, baby) => mooshroom::dimensions(baby),
            (Self::PolarBear, baby) => polar_bear::dimensions(baby),
            (Self::Panda, baby) => panda::dimensions(baby),
            (Self::Ocelot, baby) => ocelot::dimensions(baby),
            (Self::Cat, baby) => cat::dimensions(baby),
            (Self::Parrot, _) => parrot::dimensions(),
            (Self::Squid | Self::GlowSquid, baby) => squid::dimensions(self, baby),
            (Self::Cod | Self::Salmon | Self::TropicalFish | Self::Pufferfish, _) => {
                fish::dimensions(self, 1, 0)
            }
            (Self::Turtle, baby) => turtle::dimensions(self, baby),
            (Self::Dolphin, baby) => dolphin::dimensions(baby),
            (Self::Axolotl, baby) => axolotl::dimensions(baby),
            (Self::Silverfish | Self::Endermite, _) => infested::dimensions(self),
            (Self::Cow, false) => (0.9, 1.4, 1.3),
            // `Cow.BABY_DIMENSIONS`：`scalable(0.45F, 0.7F).withEyeHeight(0.69F)`。
            (Self::Cow, true) => (0.45, 0.7, 0.69),
            (Self::Zombie | Self::Husk | Self::Drowned, false) => (0.6, 1.95, 1.74),
            // `Zombie.BABY_DIMENSIONS`：`scalable(0.49F, 0.98F).withEyeHeight(0.775F)`。
            (Self::Zombie | Self::Husk | Self::Drowned, true) => (0.49, 0.98, 0.775),
            // `EntityTypes.WITCH`：`sized(0.6F, 1.95F).eyeHeight(1.62F)`。
            //
            // `EntityTypes.WITCH`: `sized(0.6F, 1.95F).eyeHeight(1.62F)`.
            (Self::Witch, _) => (0.6, 1.95, 1.62),
            // `EntityTypes.PILLAGER`：`sized(0.6F, 1.95F)`，眼高取缺省的 `height * 0.85F`。
            //
            // `EntityTypes.PILLAGER`: `sized(0.6F, 1.95F)` with the default eye height
            // `height * 0.85F`.
            (Self::Pillager, _) => (0.6, 1.95, 1.95 * 0.85),
        }
    }

    /// 是不是 `Monster` 一支（决定消失规则、经验和战利品规则）。
    pub const fn is_monster(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Husk
                | Self::Drowned
                | Self::Witch
                | Self::Pillager
                | Self::Creeper
                | Self::Spider
                | Self::CaveSpider
                | Self::Enderman
                | Self::Silverfish
                | Self::Endermite
        ) || self.is_skeleton()
    }

    /// 是不是官方 `Raider` 一支（`PatrollingMonster` 的全部子类）：女巫和掠夺者。它们共用
    /// 巡逻、`noActionTime` 每刻加 2、巡逻中不按距离消失这几条规则。
    ///
    /// Whether this is a vanilla `Raider` (every `PatrollingMonster` subclass): witches and
    /// pillagers. They share patrols, the unconditional `noActionTime += 2`, and the
    /// no-distance-despawn rule while patrolling.
    pub const fn is_raider(self) -> bool {
        matches!(self, Self::Witch | Self::Pillager)
    }

    /// 是不是 `Zombie` 一支：僵尸、尸壳、溺尸共用近战、破门、增援和转化逻辑。
    ///
    /// Whether this is a `Zombie`: zombies, husks and drowned share the melee, door
    /// breaking, reinforcement and conversion logic.
    pub const fn is_zombie(self) -> bool {
        matches!(self, Self::Zombie | Self::Husk | Self::Drowned)
    }

    /// 是不是 `AbstractSkeleton` 一支：骷髅、流浪者、沼骸共用弓 / 近战目标和装备逻辑。
    ///
    /// Whether this is an `AbstractSkeleton`: skeletons, strays and bogged share the
    /// bow / melee goals and the equipment logic.
    pub const fn is_skeleton(self) -> bool {
        matches!(self, Self::Skeleton | Self::Stray | Self::Bogged)
    }

    /// `LivingEntity.isInvertedHealAndHarm()`：`#inverted_healing_and_harm` 标签。
    ///
    /// `LivingEntity.isInvertedHealAndHarm()`: the `#inverted_healing_and_harm` tag.
    pub fn is_inverted_heal_and_harm(self) -> bool {
        self.in_entity_tag("minecraft:inverted_healing_and_harm")
    }

    /// `Entity.canFreeze()` 的类型部分：`#freeze_immune_entity_types`（流浪者）和
    /// `Skeleton.canFreeze()` 的覆写（恒假）。
    ///
    /// The type part of `Entity.canFreeze()`: `#freeze_immune_entity_types` (strays) and
    /// the `Skeleton.canFreeze()` override (always false).
    pub const fn can_freeze(self) -> bool {
        !matches!(self, Self::Skeleton | Self::Stray)
    }

    /// 目标选择按官方 `TargetGoal` 的完整规则走（`mustSee` 的视线记忆、生物也能当目标、
    /// 按真实碰撞箱判近战距离）。
    ///
    /// Whether targeting follows the full vanilla `TargetGoal` rules (`mustSee` memory,
    /// mob targets, real hitboxes for melee reach).
    pub const fn full_target_rules(self) -> bool {
        matches!(
            self,
            Self::Creeper
                | Self::Spider
                | Self::CaveSpider
                | Self::Enderman
                | Self::Pillager
                | Self::Endermite
        ) || self.is_skeleton()
    }

    /// `TargetGoal.mustSee` 的视线记忆：僵尸一系和完整规则的敌对生物都要按看不见的
    /// 刻数放弃目标；其余目标只看能不能打、在不在跟随距离内。
    ///
    /// Whether `TargetGoal.canContinueToUse` tracks how long the target has been unseen.
    /// Zombies join the hostile mobs that use the full `mustSee` memory.
    pub const fn uses_target_memory(self) -> bool {
        self.full_target_rules() || matches!(self, Self::Slime) || self.is_zombie()
    }

    /// `Mob.getAmbientSound()` 返回 null 的种类：`makeSound(null)` 不出声，也不取音高的随机数。
    ///
    /// Kinds whose `Mob.getAmbientSound()` is null: `makeSound(null)` plays nothing and
    /// draws no pitch.
    pub const fn has_no_ambient_sound(self) -> bool {
        matches!(
            self,
            Self::Creeper
                | Self::Pufferfish
                | Self::Slime
                | Self::Tadpole
                | Self::Bee
                | Self::IronGolem
        )
    }

    /// 走路声用脚下方块的 `SoundType`（`Entity.playStepSound` 的缺省实现），不是生物自己的音效。
    ///
    /// Footsteps use the `SoundType` of the block below (the default `Entity.playStepSound`).
    pub const fn uses_block_step_sound(self) -> bool {
        matches!(
            self,
            Self::Creeper
                | Self::Enderman
                | Self::Witch
                | Self::Slime
                | Self::Phantom
                | Self::Pillager
                | Self::Ocelot
                | Self::Cat
                | Self::Bee
        )
    }

    /// `Spider.onClimbable()` 读 `DATA_FLAGS_ID` 的第 0 位，攀爬物理因此改走实体标志。
    ///
    /// `Spider.onClimbable()` reads bit 0 of `DATA_FLAGS_ID`, so climbing physics
    /// reads the entity flag instead of the block below.
    pub const fn climbs_walls(self) -> bool {
        matches!(self, Self::Spider | Self::CaveSpider)
    }

    /// `EntityType.isAllowedInPeaceful()`：牛为真，敌对生物为假。
    pub const fn allowed_in_peaceful(self) -> bool {
        !self.is_enemy()
    }

    /// `MobCategory.MONSTER` 的类型：`Monster` 一支加史莱姆（它是 `AgeableMob`，只实现
    /// `Enemy`）。消失规则、和平难度、音源类别都按这个分。
    ///
    /// The `MobCategory.MONSTER` types: the `Monster` branch plus slimes (an `AgeableMob`
    /// that only implements `Enemy`). Despawning, peaceful removal and the sound category
    /// follow it.
    pub const fn is_enemy(self) -> bool {
        self.is_monster() || matches!(self, Self::Slime | Self::Phantom)
    }

    /// `EntityType.getCategory()` 的消失距离。牛是 `CREATURE`（32），
    /// 僵尸是 `MONSTER`（128 / 32）。
    pub fn despawn_distance(self) -> i32 {
        self.mob_category().facts().despawn_distance
    }

    /// `Mob.removeWhenFarAway`：牛为假，`Monster` 为真；鱿鱼不是 `Animal`，沿用缺省值真。
    ///
    /// `Mob.removeWhenFarAway`: false for cows, true for monsters; squid are not
    /// `Animal`s and keep the default of true.
    pub const fn removes_when_far_away(self) -> bool {
        // `AbstractGolem.removeWhenFarAway` 恒假。
        // `AbstractGolem.removeWhenFarAway` is always false.
        if matches!(self, Self::IronGolem | Self::SnowGolem) {
            return false;
        }
        // 鱿鱼、海豚（`AgeableWaterCreature`）、鱼（`WaterAnimal`）和蝙蝠不是 `Animal`，沿用
        // `Mob.removeWhenFarAway` 的缺省值真；美西螈由 `Mob::removes_when_far_away` 单判
        // （装过桶或起了名就不消失）；敌对生物按 `is_enemy` 判定。
        // Squid, dolphins (AgeableWaterCreature), fish (WaterAnimal) and bats keep the
        // `Mob.removeWhenFarAway` default of true; the axolotl is handled in
        // `Mob::removes_when_far_away` (never once bucketed or named); enemies follow
        // `is_enemy`.
        self.is_enemy()
            || squid::is_squid(self)
            || matches!(self, Self::Bat | Self::Dolphin | Self::Axolotl)
    }
    /// `LivingEntity.canBreatheUnderwater()`：`#can_breathe_under_water` 里已建模的水生生物。
    /// 标签里还有 `#undead`，僵尸和骷髅的溺水行为沿用旧实现，这里不改。
    ///
    /// `LivingEntity.canBreatheUnderwater()` for the modelled aquatic members of
    /// `#can_breathe_under_water`. The tag also holds `#undead`; zombie and skeleton
    /// drowning keeps its earlier behaviour and is not changed here.
    pub const fn can_breathe_under_water(self) -> bool {
        squid::is_squid(self)
            || fish::is_fish(self)
            || turtle::is_turtle(self)
            // 幻翼在 `#undead` 里，`#can_breathe_under_water` 收了整个标签。
            // Phantoms are `#undead`, which `#can_breathe_under_water` includes.
            || matches!(self, Self::Axolotl | Self::Frog | Self::Phantom)
    }

    /// 是不是官方 `WaterAnimal`（鱼）：有氧气处理、不吃水流、生成时不查液体，
    /// 但不是 `AgeableMob`，没有年龄。
    ///
    /// Whether this is a vanilla `WaterAnimal` (fish): air supply, no fluid pushing,
    /// no liquid check when spawning, and no age because it is not an `AgeableMob`.
    pub const fn is_water_animal(self) -> bool {
        fish::is_fish(self)
    }

    /// 是不是 `AgeableMob`（牛、猪、羊、鸡、鱿鱼）：写 `Age` 三个键、会随年龄长大。
    ///
    /// Whether this is an `AgeableMob` (cow, pig, sheep, chicken, squid): saves the three
    /// `Age` keys and grows up.
    pub const fn is_ageable(self) -> bool {
        !self.is_monster()
            && !fish::is_fish(self)
            && !matches!(
                self,
                Self::Bat | Self::Phantom | Self::IronGolem | Self::SnowGolem
            )
    }

    /// `AgeableMob.canBeABaby()`：青蛙覆盖成假。它的年龄照样能被设成负数（组里后来的
    /// 成员按概率 `setAge(-24000)`），但不算幼年、不长大，`Age` 三个键也不存。
    ///
    /// `AgeableMob.canBeABaby()`, overridden to false by frogs. Their age can still go
    /// negative (later group members roll `setAge(-24000)`), but they are not babies, never
    /// grow up and do not save the three `Age` keys.
    pub const fn can_be_a_baby(self) -> bool {
        !matches!(self, Self::Frog | Self::Parrot)
    }

    /// 是不是官方 `AgeableWaterCreature` 一支（鱿鱼、发光鱿鱼、海豚）：不吃水流，
    /// 生成时不查液体。海豚把 `handleAirSupply` 覆写为空实现，氧气单独处理。
    ///
    /// Whether this is on vanilla's `AgeableWaterCreature` branch (squid, glow squid and
    /// dolphins): currents do not push it and spawning skips the liquid check. A dolphin
    /// overrides `handleAirSupply` to a no-op, so its air is handled separately.
    pub const fn is_ageable_water_creature(self) -> bool {
        squid::is_squid(self) || matches!(self, Self::Dolphin)
    }

    /// `Entity.isPushedByFluid()`：`AgeableWaterCreature` 覆盖成假。
    ///
    /// `Entity.isPushedByFluid()`, overridden to false by `AgeableWaterCreature`.
    pub const fn is_pushed_by_fluid(self) -> bool {
        !(self.is_ageable_water_creature()
            || self.is_water_animal()
            || turtle::is_turtle(self)
            || matches!(self, Self::Frog))
    }

    /// 是不是官方 `Animal`（有求爱和喂食）：非怪物里除掉 `AgeableWaterCreature` 一支。
    ///
    /// Whether this is a vanilla `Animal` (love and feeding): every non-monster except the
    /// `AgeableWaterCreature` branch.
    pub const fn is_animal(self) -> bool {
        !self.is_monster()
            && !self.is_ageable_water_creature()
            && !self.is_water_animal()
            && !matches!(
                self,
                Self::Bat | Self::Phantom | Self::IronGolem | Self::SnowGolem
            )
    }

    /// `Entity.getSwimSound`：鱼覆盖成 `entity.fish.swim`，海龟是 `entity.turtle.swim`，其余是
    /// `entity.generic.swim`。
    ///
    /// `Entity.getSwimSound`: fish override it with `entity.fish.swim`, turtles with
    /// `entity.turtle.swim`.
    pub const fn swim_sound(self) -> &'static str {
        if self.is_water_animal() {
            fish::SWIM_SOUND
        } else if turtle::is_turtle(self) {
            "minecraft:entity.turtle.swim"
        } else if matches!(self, Self::Dolphin) {
            "minecraft:entity.dolphin.swim"
        } else if matches!(self, Self::Axolotl) {
            "minecraft:entity.axolotl.swim"
        } else {
            "minecraft:entity.generic.swim"
        }
    }

    /// `Entity.getMovementEmission().emitsSounds()`：鱿鱼覆盖成 `EVENTS`，移动不出声。
    ///
    /// `Entity.getMovementEmission().emitsSounds()`: squid override it to `EVENTS` and
    /// move silently.
    pub const fn emits_movement_sounds(self) -> bool {
        !squid::is_squid(self) && !matches!(self, Self::Bat | Self::Silverfish | Self::Endermite)
    }

    /// `LivingEntity.isPushable()`：蝙蝠覆写成假，不推别人也不被推。
    ///
    /// `LivingEntity.isPushable()`, overridden to false by bats, which neither push nor
    /// get pushed.
    pub const fn is_pushable(self) -> bool {
        !matches!(self, Self::Bat)
    }

    /// 把 `checkFallDamage` 覆写成空方法的类型（鹦鹉、蝙蝠和幻翼）。
    ///
    /// Types that override `checkFallDamage` with an empty body (parrots, bats and
    /// phantoms).
    pub const fn ignores_fall_damage_check(self) -> bool {
        matches!(self, Self::Parrot | Self::Bat | Self::Phantom | Self::Bee)
    }

    /// `SpawnPlacements` 里登记成 `IN_WATER` 的类型。
    ///
    /// Types registered as `IN_WATER` in `SpawnPlacements`.
    pub const fn spawns_in_water(self) -> bool {
        squid::is_squid(self)
            || fish::is_fish(self)
            || matches!(self, Self::Dolphin | Self::Axolotl)
    }

    /// `Mob.xpReward`：牛由 `Animal.getBaseExperienceReward` 抽，僵尸固定 5。
    pub const fn xp_reward(self) -> i32 {
        if matches!(self, Self::Endermite) {
            return mc_bamboo_data::entity_types::ENDERMITE_XP_REWARD;
        }
        // `Phantom` 构造函数里也是 `xpReward = 5`。
        // The `Phantom` constructor sets `xpReward = 5` as well.
        if self.is_monster() || matches!(self, Self::Phantom) {
            5
        } else {
            0
        }
    }

    /// `LivingEntity.dropAllDeathLoot` 用的战利品表。
    pub const fn loot_table(self) -> &'static str {
        match self {
            Self::Cow => "minecraft:entities/cow",
            Self::Mooshroom => "minecraft:entities/mooshroom",
            Self::Rabbit => "minecraft:entities/rabbit",
            Self::Fox => "minecraft:entities/fox",
            Self::Wolf => "minecraft:entities/wolf",
            Self::Pig => "minecraft:entities/pig",
            Self::Sheep => "minecraft:entities/sheep",
            Self::Chicken => "minecraft:entities/chicken",
            Self::PolarBear => "minecraft:entities/polar_bear",
            Self::Panda => "minecraft:entities/panda",
            Self::Ocelot => "minecraft:entities/ocelot",
            Self::Cat => "minecraft:entities/cat",
            Self::Parrot => "minecraft:entities/parrot",
            Self::Zombie => "minecraft:entities/zombie",
            Self::Husk => "minecraft:entities/husk",
            Self::Skeleton => "minecraft:entities/skeleton",
            Self::Stray => "minecraft:entities/stray",
            Self::Bogged => "minecraft:entities/bogged",
            Self::Slime => "minecraft:entities/slime",
            Self::Drowned => "minecraft:entities/drowned",
            Self::Witch => "minecraft:entities/witch",
            Self::Pillager => "minecraft:entities/pillager",
            Self::Creeper => "minecraft:entities/creeper",
            Self::Spider => "minecraft:entities/spider",
            Self::CaveSpider => "minecraft:entities/cave_spider",
            Self::Enderman => "minecraft:entities/enderman",
            Self::Squid => "minecraft:entities/squid",
            Self::GlowSquid => "minecraft:entities/glow_squid",
            Self::Cod => "minecraft:entities/cod",
            Self::Salmon => "minecraft:entities/salmon",
            Self::TropicalFish => "minecraft:entities/tropical_fish",
            Self::Pufferfish => "minecraft:entities/pufferfish",
            Self::Turtle => "minecraft:entities/turtle",
            Self::Dolphin => "minecraft:entities/dolphin",
            Self::Axolotl => "minecraft:entities/axolotl",
            Self::Horse => "minecraft:entities/horse",
            Self::Donkey => "minecraft:entities/donkey",
            Self::Mule => "minecraft:entities/mule",
            Self::Llama => "minecraft:entities/llama",
            Self::Armadillo => "minecraft:entities/armadillo",
            Self::Goat => "minecraft:entities/goat",
            Self::Frog => "minecraft:entities/frog",
            Self::Tadpole => "minecraft:entities/tadpole",
            Self::Bat => "minecraft:entities/bat",
            Self::Phantom => "minecraft:entities/phantom",
            Self::Camel => "minecraft:entities/camel",
            Self::Bee => "minecraft:entities/bee",
            Self::IronGolem => "minecraft:entities/iron_golem",
            Self::SnowGolem => "minecraft:entities/snow_golem",
            Self::Silverfish => "minecraft:entities/silverfish",
            Self::Endermite => "minecraft:entities/endermite",
        }
    }

    /// `EntityTypeTags.BURN_IN_DAYLIGHT` 加 `Zombie.isSunSensitive()`：
    /// 尸壳不烧，僵尸、溺尸和骷髅一支烧。
    pub const fn burns_in_daylight(self) -> bool {
        matches!(self, Self::Zombie | Self::Drowned | Self::Phantom) || self.is_skeleton()
    }

    /// `Mob.getAmbientSoundInterval()`：牛覆写成 120，僵尸用缺省 80。
    pub const fn ambient_sound_interval(self) -> i32 {
        if matches!(self, Self::Ocelot) {
            900
        } else if self.is_equine() {
            400
        } else if self.is_enemy() || matches!(self, Self::Bat) {
            // `Mob.getAmbientSoundInterval()` 的缺省值；蝙蝠没有覆写。
            // The `Mob.getAmbientSoundInterval()` default, which bats keep.
            80
        } else if turtle::is_turtle(self) {
            200
        } else {
            120
        }
    }

    /// `Mob.getSoundVolume()`。牛 0.4，僵尸 1.0（`Monster` 用 `getSoundVolume` 缺省）。
    pub const fn sound_volume(self) -> f32 {
        if self.is_equine() {
            0.8
        } else if matches!(self, Self::Bat) {
            // `Bat.getSoundVolume()`。
            0.1
        } else if matches!(
            self,
            Self::Cow | Self::Mooshroom | Self::Squid | Self::GlowSquid | Self::Wolf | Self::Bee
        ) {
            0.4
        } else {
            1.0
        }
    }

    /// 四种音效按官方类分派，僵尸和尸壳各有自己的事件名。幼崽叫声不同的
    /// （猪、鸡、北极熊）返回 `None`，由 `living::ambient_sound` 按年龄分派。
    ///
    /// The four sounds are dispatched by class; zombies and husks have their own events.
    /// Kinds whose cubs sound different (pigs, chickens, polar bears) return `None` and let
    /// `living::ambient_sound` pick by age.
    pub const fn ambient_sound(self) -> Option<&'static str> {
        match self {
            Self::Rabbit => Some("minecraft:entity.rabbit.ambient"),
            Self::Fox => Some("minecraft:entity.fox.ambient"),
            // 狼的叫声按叫声品种和幼年换，见 [`wolf::sound`]。
            // Wolf sounds depend on the sound variant and age; see [`wolf::sound`].
            Self::Wolf => Some("minecraft:entity.wolf.ambient"),
            Self::Cow
            | Self::Mooshroom
            | Self::Pig
            | Self::Chicken
            | Self::Slime
            | Self::Creeper
            | Self::PolarBear
            | Self::Panda => None,
            Self::Sheep => Some("minecraft:entity.sheep.ambient"),
            Self::Zombie => Some("minecraft:entity.zombie.ambient"),
            Self::Husk => Some("minecraft:entity.husk.ambient"),
            Self::Skeleton => Some("minecraft:entity.skeleton.ambient"),
            Self::Stray => Some("minecraft:entity.stray.ambient"),
            Self::Bogged => Some("minecraft:entity.bogged.ambient"),
            // 溺尸的叫声分水上、水下两种，由 [`drowned::sound`] 按 `isInWater` 选。
            //
            // Drowned pick a water or land variant in [`drowned::sound`] by `isInWater`.
            Self::Drowned => None,
            Self::Witch => Some("minecraft:entity.witch.ambient"),
            Self::Pillager => Some("minecraft:entity.pillager.ambient"),
            Self::Enderman => Some("minecraft:entity.enderman.ambient"),
            Self::Spider | Self::CaveSpider => Some("minecraft:entity.spider.ambient"),
            Self::Squid | Self::GlowSquid => Some(squid::ambient_sound(self)),
            Self::Cod | Self::Salmon | Self::TropicalFish => Some(fish::ambient_sound(self)),
            Self::Pufferfish => None,
            Self::Turtle => None,
            // 海豚和水陆两栖的美西螈叫声看环境，见 `living::ambient_voice`。
            // Dolphin and amphibious-axolotl ambience depend on the environment; see
            // `living::ambient_voice`.
            Self::Dolphin => Some("minecraft:entity.dolphin.ambient"),
            Self::Axolotl => Some("minecraft:entity.axolotl.idle_air"),
            Self::Horse => Some("minecraft:entity.horse.ambient"),
            Self::Donkey => Some("minecraft:entity.donkey.ambient"),
            Self::Mule => Some("minecraft:entity.mule.ambient"),
            Self::Llama => Some("minecraft:entity.llama.ambient"),
            Self::Armadillo => Some("minecraft:entity.armadillo.ambient"),
            Self::Goat => Some("minecraft:entity.goat.ambient"),
            Self::Frog => Some("minecraft:entity.frog.ambient"),
            Self::Tadpole => None,
            Self::Bat => Some("minecraft:entity.bat.ambient"),
            Self::Phantom => Some("minecraft:entity.phantom.ambient"),
            Self::Ocelot => Some("minecraft:entity.ocelot.ambient"),
            Self::Cat => Some("minecraft:entity.cat.stray_ambient"),
            // 鹦鹉的叫声有模仿分支，由 [`living::play_ambient_sound`] 拦截。
            // Parrot ambient sounds have an imitation branch handled in
            // [`living::play_ambient_sound`].
            Self::Parrot => Some("minecraft:entity.parrot.ambient"),
            Self::Camel => Some("minecraft:entity.camel.ambient"),
            // `Bee.getAmbientSound()` 返回 null：不出环境音也不抽音高的随机数。
            Self::Bee => None,
            Self::IronGolem => None,
            Self::SnowGolem => Some("minecraft:entity.snow_golem.ambient"),
            Self::Silverfish => Some("minecraft:entity.silverfish.ambient"),
            Self::Endermite => Some("minecraft:entity.endermite.ambient"),
        }
    }

    pub const fn hurt_sound(self) -> Option<&'static str> {
        match self {
            Self::Rabbit => Some("minecraft:entity.rabbit.hurt"),
            Self::Fox => Some("minecraft:entity.fox.hurt"),
            Self::Wolf => Some("minecraft:entity.wolf.hurt"),
            Self::Cow | Self::Mooshroom | Self::Pig | Self::Chicken | Self::Slime => None,
            Self::Sheep => Some("minecraft:entity.sheep.hurt"),
            Self::PolarBear => Some("minecraft:entity.polar_bear.hurt"),
            Self::Panda => Some("minecraft:entity.panda.hurt"),
            Self::Ocelot => Some("minecraft:entity.ocelot.hurt"),
            Self::Cat => None,
            Self::Parrot => Some("minecraft:entity.parrot.hurt"),
            Self::Zombie => Some("minecraft:entity.zombie.hurt"),
            Self::Husk => Some("minecraft:entity.husk.hurt"),
            Self::Skeleton => Some("minecraft:entity.skeleton.hurt"),
            Self::Stray => Some("minecraft:entity.stray.hurt"),
            Self::Bogged => Some("minecraft:entity.bogged.hurt"),
            Self::Drowned => None,
            Self::Witch => Some("minecraft:entity.witch.hurt"),
            Self::Pillager => Some("minecraft:entity.pillager.hurt"),
            Self::Creeper => Some("minecraft:entity.creeper.hurt"),
            Self::Enderman => Some("minecraft:entity.enderman.hurt"),
            Self::Spider | Self::CaveSpider => Some("minecraft:entity.spider.hurt"),
            Self::Squid | Self::GlowSquid => Some(squid::hurt_sound(self)),
            Self::Cod | Self::Salmon | Self::TropicalFish | Self::Pufferfish => {
                Some(fish::hurt_sound(self))
            }
            Self::Turtle => Some("minecraft:entity.turtle.hurt"),
            Self::Dolphin => Some("minecraft:entity.dolphin.hurt"),
            Self::Axolotl => Some("minecraft:entity.axolotl.hurt"),
            Self::Horse => Some("minecraft:entity.horse.hurt"),
            Self::Donkey => Some("minecraft:entity.donkey.hurt"),
            Self::Mule => Some("minecraft:entity.mule.hurt"),
            Self::Llama => Some("minecraft:entity.llama.hurt"),
            Self::Armadillo => Some("minecraft:entity.armadillo.hurt"),
            Self::Goat => Some("minecraft:entity.goat.hurt"),
            Self::Frog => Some("minecraft:entity.frog.hurt"),
            Self::Tadpole => Some("minecraft:entity.tadpole.hurt"),
            Self::Bat => Some("minecraft:entity.bat.hurt"),
            Self::Phantom => Some("minecraft:entity.phantom.hurt"),
            Self::Camel => Some("minecraft:entity.camel.hurt"),
            Self::Bee => Some("minecraft:entity.bee.hurt"),
            Self::IronGolem => Some("minecraft:entity.iron_golem.hurt"),
            Self::SnowGolem => Some("minecraft:entity.snow_golem.hurt"),
            Self::Silverfish => Some("minecraft:entity.silverfish.hurt"),
            Self::Endermite => Some("minecraft:entity.endermite.hurt"),
        }
    }

    pub const fn death_sound(self) -> Option<&'static str> {
        match self {
            Self::Rabbit => Some("minecraft:entity.rabbit.death"),
            Self::Fox => Some("minecraft:entity.fox.death"),
            Self::Wolf => Some("minecraft:entity.wolf.death"),
            Self::Cow | Self::Mooshroom | Self::Pig | Self::Chicken | Self::Slime => None,
            Self::Sheep => Some("minecraft:entity.sheep.death"),
            Self::PolarBear => Some("minecraft:entity.polar_bear.death"),
            Self::Panda => Some("minecraft:entity.panda.death"),
            Self::Ocelot => Some("minecraft:entity.ocelot.death"),
            Self::Cat => None,
            Self::Parrot => Some("minecraft:entity.parrot.death"),
            Self::Zombie => Some("minecraft:entity.zombie.death"),
            Self::Husk => Some("minecraft:entity.husk.death"),
            Self::Skeleton => Some("minecraft:entity.skeleton.death"),
            Self::Stray => Some("minecraft:entity.stray.death"),
            Self::Bogged => Some("minecraft:entity.bogged.death"),
            Self::Drowned => None,
            Self::Witch => Some("minecraft:entity.witch.death"),
            Self::Pillager => Some("minecraft:entity.pillager.death"),
            Self::Creeper => Some("minecraft:entity.creeper.death"),
            Self::Enderman => Some("minecraft:entity.enderman.death"),
            Self::Spider | Self::CaveSpider => Some("minecraft:entity.spider.death"),
            Self::Squid | Self::GlowSquid => Some(squid::death_sound(self)),
            Self::Cod | Self::Salmon | Self::TropicalFish | Self::Pufferfish => {
                Some(fish::death_sound(self))
            }
            Self::Turtle => Some("minecraft:entity.turtle.death"),
            Self::Dolphin => Some("minecraft:entity.dolphin.death"),
            Self::Axolotl => Some("minecraft:entity.axolotl.death"),
            Self::Horse => Some("minecraft:entity.horse.death"),
            Self::Donkey => Some("minecraft:entity.donkey.death"),
            Self::Mule => Some("minecraft:entity.mule.death"),
            Self::Llama => Some("minecraft:entity.llama.death"),
            Self::Armadillo => Some("minecraft:entity.armadillo.death"),
            Self::Goat => Some("minecraft:entity.goat.death"),
            Self::Frog => Some("minecraft:entity.frog.death"),
            Self::Tadpole => Some("minecraft:entity.tadpole.death"),
            Self::Bat => Some("minecraft:entity.bat.death"),
            Self::Phantom => Some("minecraft:entity.phantom.death"),
            Self::Camel => Some("minecraft:entity.camel.death"),
            Self::Bee => Some("minecraft:entity.bee.death"),
            Self::IronGolem => Some("minecraft:entity.iron_golem.death"),
            Self::SnowGolem => Some("minecraft:entity.snow_golem.death"),
            Self::Silverfish => Some("minecraft:entity.silverfish.death"),
            Self::Endermite => Some("minecraft:entity.endermite.death"),
        }
    }

    pub const fn step_sound(self) -> Option<&'static str> {
        match self {
            Self::Rabbit | Self::Fox | Self::Wolf | Self::Slime => None,
            Self::Cow
            | Self::Mooshroom
            | Self::Pig
            | Self::Chicken
            | Self::Creeper
            | Self::Enderman => None,
            Self::Sheep => Some("minecraft:entity.sheep.step"),
            Self::PolarBear => Some("minecraft:entity.polar_bear.step"),
            Self::Panda => Some("minecraft:entity.panda.step"),
            Self::Zombie => Some("minecraft:entity.zombie.step"),
            Self::Husk => Some("minecraft:entity.husk.step"),
            Self::Skeleton => Some("minecraft:entity.skeleton.step"),
            Self::Stray => Some("minecraft:entity.stray.step"),
            Self::Bogged => Some("minecraft:entity.bogged.step"),
            Self::Drowned => Some("minecraft:entity.drowned.step"),
            // 女巫没覆写 `getStepSound`，走方块自己的脚步声（和史莱姆一样）。
            //
            // A witch does not override `getStepSound`; the block's own step sound plays.
            Self::Witch | Self::Pillager => None,
            Self::Spider | Self::CaveSpider => Some("minecraft:entity.spider.step"),
            // `MovementEmission.EVENTS`：没有脚步声。
            // `MovementEmission.EVENTS`: no step sound.
            Self::Squid | Self::GlowSquid => None,
            // `AbstractFish.playStepSound` 是空实现。
            // `AbstractFish.playStepSound` is empty.
            Self::Cod | Self::Salmon | Self::TropicalFish | Self::Pufferfish => None,
            Self::Turtle => Some("minecraft:entity.turtle.shamble"),
            // 海豚和美西螈游泳时不发脚步声（`playStepSound` 不走一般路径）。
            // Neither emits a ground step sound while swimming.
            Self::Dolphin | Self::Axolotl => None,
            Self::Horse => Some("minecraft:entity.horse.step"),
            Self::Donkey => Some("minecraft:entity.horse.step"),
            Self::Mule => Some("minecraft:entity.horse.step"),
            Self::Llama => Some("minecraft:entity.llama.step"),
            Self::Armadillo => Some("minecraft:entity.armadillo.step"),
            Self::Goat => Some("minecraft:entity.goat.step"),
            Self::Frog => Some("minecraft:entity.frog.step"),
            // `MovementEmission.EVENTS`：没有脚步声。
            // `MovementEmission.EVENTS`: no step sound.
            Self::Tadpole | Self::Bat | Self::Ocelot | Self::Cat => None,
            Self::Parrot => Some("minecraft:entity.parrot.step"),
            // 幻翼没覆写 `playStepSound`，用方块自己的脚步声。
            // Phantoms keep the default `playStepSound`: the block's own step sound.
            Self::Phantom => None,
            Self::Camel => Some("minecraft:entity.camel.step"),
            // `Bee.playStepSound` 是空实现。
            Self::Bee => None,
            Self::IronGolem => Some("minecraft:entity.iron_golem.step"),
            Self::SnowGolem => None,
            Self::Silverfish => Some("minecraft:entity.silverfish.step"),
            Self::Endermite => Some("minecraft:entity.endermite.step"),
        }
    }

    /// `SpawnEggItem` 的物品 ID；牛刷怪蛋已有，僵尸/尸壳的 ID 也在数据表里。
    pub const fn spawn_egg(self) -> i32 {
        use mc_bamboo_data::items;
        match self {
            Self::Cow => items::COW_SPAWN_EGG,
            Self::Mooshroom => items::MOOSHROOM_SPAWN_EGG,
            Self::Rabbit => items::RABBIT_SPAWN_EGG,
            Self::Fox => items::FOX_SPAWN_EGG,
            Self::Wolf => items::WOLF_SPAWN_EGG,
            Self::Pig => items::PIG_SPAWN_EGG,
            Self::Sheep => items::SHEEP_SPAWN_EGG,
            Self::Chicken => items::CHICKEN_SPAWN_EGG,
            Self::PolarBear => items::POLAR_BEAR_SPAWN_EGG,
            Self::Panda => items::PANDA_SPAWN_EGG,
            Self::Ocelot => items::OCELOT_SPAWN_EGG,
            Self::Cat => items::CAT_SPAWN_EGG,
            Self::Parrot => items::PARROT_SPAWN_EGG,
            Self::Zombie => items::ZOMBIE_SPAWN_EGG,
            Self::Husk => items::HUSK_SPAWN_EGG,
            Self::Skeleton => items::SKELETON_SPAWN_EGG,
            Self::Stray => items::STRAY_SPAWN_EGG,
            Self::Bogged => items::BOGGED_SPAWN_EGG,
            Self::Slime => items::SLIME_SPAWN_EGG,
            Self::Drowned => items::DROWNED_SPAWN_EGG,
            Self::Witch => items::WITCH_SPAWN_EGG,
            Self::Pillager => items::PILLAGER_SPAWN_EGG,
            Self::Creeper => items::CREEPER_SPAWN_EGG,
            Self::Spider => items::SPIDER_SPAWN_EGG,
            Self::CaveSpider => items::CAVE_SPIDER_SPAWN_EGG,
            Self::Enderman => items::ENDERMAN_SPAWN_EGG,
            Self::Squid => items::SQUID_SPAWN_EGG,
            Self::GlowSquid => items::GLOW_SQUID_SPAWN_EGG,
            Self::Cod => items::COD_SPAWN_EGG,
            Self::Salmon => items::SALMON_SPAWN_EGG,
            Self::TropicalFish => items::TROPICAL_FISH_SPAWN_EGG,
            Self::Pufferfish => items::PUFFERFISH_SPAWN_EGG,
            Self::Turtle => items::TURTLE_SPAWN_EGG,
            Self::Dolphin => items::DOLPHIN_SPAWN_EGG,
            Self::Axolotl => items::AXOLOTL_SPAWN_EGG,
            Self::Horse => items::HORSE_SPAWN_EGG,
            Self::Donkey => items::DONKEY_SPAWN_EGG,
            Self::Mule => items::MULE_SPAWN_EGG,
            Self::Llama => items::LLAMA_SPAWN_EGG,
            Self::Armadillo => items::ARMADILLO_SPAWN_EGG,
            Self::Goat => items::GOAT_SPAWN_EGG,
            Self::Frog => items::FROG_SPAWN_EGG,
            Self::Tadpole => items::TADPOLE_SPAWN_EGG,
            Self::Bat => items::BAT_SPAWN_EGG,
            Self::Phantom => items::PHANTOM_SPAWN_EGG,
            Self::Camel => items::CAMEL_SPAWN_EGG,
            Self::Bee => items::BEE_SPAWN_EGG,
            Self::IronGolem => items::IRON_GOLEM_SPAWN_EGG,
            Self::SnowGolem => items::SNOW_GOLEM_SPAWN_EGG,
            Self::Silverfish => items::SILVERFISH_SPAWN_EGG,
            Self::Endermite => items::ENDERMITE_SPAWN_EGG,
        }
    }

    /// `/summon` 成功反馈里的翻译键。
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::Cow => "entity.minecraft.cow",
            Self::Mooshroom => "entity.minecraft.mooshroom",
            Self::Rabbit => "entity.minecraft.rabbit",
            Self::Fox => "entity.minecraft.fox",
            Self::Wolf => "entity.minecraft.wolf",
            Self::Pig => "entity.minecraft.pig",
            Self::Sheep => "entity.minecraft.sheep",
            Self::Chicken => "entity.minecraft.chicken",
            Self::PolarBear => "entity.minecraft.polar_bear",
            Self::Panda => "entity.minecraft.panda",
            Self::Ocelot => "entity.minecraft.ocelot",
            Self::Cat => "entity.minecraft.cat",
            Self::Parrot => "entity.minecraft.parrot",
            Self::Zombie => "entity.minecraft.zombie",
            Self::Husk => "entity.minecraft.husk",
            Self::Skeleton => "entity.minecraft.skeleton",
            Self::Stray => "entity.minecraft.stray",
            Self::Bogged => "entity.minecraft.bogged",
            Self::Slime => "entity.minecraft.slime",
            Self::Drowned => "entity.minecraft.drowned",
            Self::Witch => "entity.minecraft.witch",
            Self::Pillager => "entity.minecraft.pillager",
            Self::Creeper => "entity.minecraft.creeper",
            Self::Spider => "entity.minecraft.spider",
            Self::CaveSpider => "entity.minecraft.cave_spider",
            Self::Enderman => "entity.minecraft.enderman",
            Self::Squid => "entity.minecraft.squid",
            Self::GlowSquid => "entity.minecraft.glow_squid",
            Self::Cod => "entity.minecraft.cod",
            Self::Salmon => "entity.minecraft.salmon",
            Self::TropicalFish => "entity.minecraft.tropical_fish",
            Self::Pufferfish => "entity.minecraft.pufferfish",
            Self::Turtle => "entity.minecraft.turtle",
            Self::Dolphin => "entity.minecraft.dolphin",
            Self::Axolotl => "entity.minecraft.axolotl",
            Self::Horse => "entity.minecraft.horse",
            Self::Donkey => "entity.minecraft.donkey",
            Self::Mule => "entity.minecraft.mule",
            Self::Llama => "entity.minecraft.llama",
            Self::Armadillo => "entity.minecraft.armadillo",
            Self::Goat => "entity.minecraft.goat",
            Self::Frog => "entity.minecraft.frog",
            Self::Tadpole => "entity.minecraft.tadpole",
            Self::Bat => "entity.minecraft.bat",
            Self::Phantom => "entity.minecraft.phantom",
            Self::Camel => "entity.minecraft.camel",
            Self::Bee => "entity.minecraft.bee",
            Self::IronGolem => "entity.minecraft.iron_golem",
            Self::SnowGolem => "entity.minecraft.snow_golem",
            Self::Silverfish => "entity.minecraft.silverfish",
            Self::Endermite => "entity.minecraft.endermite",
        }
    }

    /// 幼年是不是用 `Zombie.DATA_BABY_ID`（布尔元数据），而不是牛的 `age < 0`。
    pub const fn uses_baby_metadata(self) -> bool {
        self.is_zombie()
    }

    /// 按 `minecraft:entity_type` 注册表 ID 找种类，Bamboo 没建模的返回 `None`。
    ///
    /// Looks a kind up by `minecraft:entity_type` registry id; `None` for types
    /// Bamboo does not model.
    pub fn by_entity_type(entity_type: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.entity_type() == entity_type)
    }

    /// `EntityType.getCategory()`。
    ///
    /// The vanilla `EntityType.getCategory()` of this kind.
    pub const fn mob_category(self) -> mc_bamboo_data::spawning::MobCategory {
        use mc_bamboo_data::spawning::MobCategory;
        match self {
            Self::Squid => MobCategory::WaterCreature,
            Self::Dolphin => MobCategory::WaterCreature,
            Self::Axolotl => MobCategory::Axolotls,
            Self::GlowSquid => MobCategory::UndergroundWaterCreature,
            Self::Cod | Self::Salmon | Self::TropicalFish | Self::Pufferfish => {
                MobCategory::WaterAmbient
            }
            Self::Bat => MobCategory::Ambient,
            _ if self.is_enemy() => MobCategory::Monster,
            _ => MobCategory::Creature,
        }
    }

    /// `EntityType.canSpawnFarFromPlayer()`：`EntityType.Builder` 的缺省是
    /// `CREATURE | MISC`，牛为真、怪物和水生生物为假。
    ///
    /// `EntityType.canSpawnFarFromPlayer()`: the builder default is `CREATURE | MISC`,
    /// so cows may spawn far from players and monsters and aquatic mobs may not.
    pub const fn can_spawn_far_from_player(self) -> bool {
        use mc_bamboo_data::spawning::MobCategory;
        // 马科也是 `CREATURE`。
        // Equines are `CREATURE` as well.
        // 掠夺者在 `EntityType.Builder` 上显式调了 `canSpawnFarFromPlayer()`。
        // Pillagers call `canSpawnFarFromPlayer()` on their `EntityType.Builder`.
        matches!(
            self.mob_category(),
            MobCategory::Creature | MobCategory::Misc
        ) || matches!(self, Self::Pillager)
    }

    /// `EntityType.fireImmune()`：已支持的自然生成生物均不防火。
    ///
    /// `EntityType.fireImmune()`: all modelled natural spawns are vulnerable to fire.
    pub const fn fire_immune(self) -> bool {
        false
    }

    /// `LivingEntity.getMaxAirSupply()`：海豚 4800、美西螈 6000，其余 300。
    ///
    /// `LivingEntity.getMaxAirSupply()`: 4800 for dolphins, 6000 for axolotls, 300 otherwise.
    pub const fn max_air_supply(self) -> i32 {
        match self {
            Self::Dolphin => 4800,
            Self::Axolotl => 6000,
            _ => 300,
        }
    }

    /// `LivingEntity.increaseAirSupply(current)`：海豚整支覆写成直接回满，
    /// 其余是 `min(current + 4, max)`。
    ///
    /// `LivingEntity.increaseAirSupply(current)`: dolphins override it to return the
    /// maximum outright; everyone else uses `min(current + 4, max)`.
    pub const fn increase_air_supply(self, current: i32) -> i32 {
        if matches!(self, Self::Dolphin) {
            self.max_air_supply()
        } else {
            let filled = current + 4;
            if filled < self.max_air_supply() {
                filled
            } else {
                self.max_air_supply()
            }
        }
    }

    /// `EntityType.Builder.immuneTo(tag)`：`isBlockDangerous` 对这个标签里的方块恒假。
    /// 流浪者是 `#stray_immune_to`（细雪），所以能在细雪里生成。
    ///
    /// `EntityType.Builder.immuneTo(tag)`: `isBlockDangerous` is false for blocks in the
    /// tag. Strays use `#stray_immune_to` (powder snow) and may spawn inside it.
    pub const fn immune_to(self) -> Option<mc_bamboo_data::BlockTag> {
        match self {
            Self::Stray => Some(mc_bamboo_data::tags::block_tags::STRAY_IMMUNE_TO),
            _ => None,
        }
    }

    /// `LivingEntity.getWaterSlowDown()`：缺省 0.8，北极熊覆写成 0.98。
    ///
    /// `LivingEntity.getWaterSlowDown()`: 0.8 by default, 0.98 for polar bears.
    pub const fn water_slow_down(self) -> f32 {
        if matches!(self, Self::PolarBear) {
            0.98
        } else {
            0.8
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
    /// 羊驼口水，`minecraft:spit`。
    ///
    /// Llama spit, `minecraft:spit`.
    Spit,
    /// 蜜蜂蜇刺，`minecraft:sting`。
    Sting,
    Thrown,
    Starve,
    Thorns,
    Magic,
    /// 间接魔法（药水 / 唤魔者尖牙等），`minecraft:indirect_magic`。
    ///
    /// Indirect magic (potions, evoker fangs, etc.), `minecraft:indirect_magic`.
    IndirectMagic,
    Wither,
    Explosion,
    /// 海豚 / 美西螈 / 鱼离水干死，`minecraft:dry_out`。
    ///
    /// Dolphins, axolotls and fish drying out: `minecraft:dry_out`.
    DryOut,
    /// `minecraft:player_explosion`：有直接和间接来源实体的爆炸（苦力怕炸人就是这种）。
    ///
    /// `minecraft:player_explosion`: an explosion with a direct and a causing entity,
    /// which is what a creeper blast is.
    PlayerExplosion,
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
            Self::Spit => "minecraft:spit",
            Self::Sting => "minecraft:sting",
            Self::Thrown => "minecraft:thrown",
            Self::Starve => "minecraft:starve",
            Self::Thorns => "minecraft:thorns",
            Self::Magic => "minecraft:magic",
            Self::IndirectMagic => "minecraft:indirect_magic",
            Self::Wither => "minecraft:wither",
            Self::Explosion => "minecraft:explosion",
            Self::DryOut => "minecraft:dry_out",
            Self::PlayerExplosion => "minecraft:player_explosion",
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
            Self::MobAttack | Self::MobAttackNoAggro | Self::Spit => "mob",
            Self::Sting => "sting",
            Self::Arrow => "arrow",
            Self::Thrown => "thrown",
            Self::Starve => "starve",
            Self::Thorns => "thorns",
            Self::Magic => "magic",
            Self::IndirectMagic => "indirectMagic",
            Self::Wither => "wither",
            Self::Explosion => "explosion",
            Self::DryOut => "dryout",
            Self::PlayerExplosion => "explosion.player",
        }
    }

    /// 按官方短名找一种伤害。未知名字返回 `None`，调用方按解析失败处理。
    ///
    /// Finds a kind by its short official name. Unknown names return `None`.
    pub fn by_name(name: &str) -> Option<Self> {
        let all = [
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
            Self::Spit,
            Self::Sting,
            Self::Thrown,
            Self::Starve,
            Self::Thorns,
            Self::Magic,
            Self::IndirectMagic,
            Self::Wither,
            Self::Explosion,
            Self::DryOut,
            Self::PlayerExplosion,
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
                | Self::Spit
                | Self::Thrown
                | Self::Thorns
                | Self::IndirectMagic
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
                | Self::Magic
                | Self::IndirectMagic
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
        matches!(self, Self::Explosion | Self::PlayerExplosion)
    }

    /// `DamageType.exhaustion`：受伤后累加的饥饿消耗。
    pub const fn exhaustion(self) -> f32 {
        const TENTH: f32 = 0.1;
        match self {
            Self::PlayerAttack
            | Self::MobAttack
            | Self::MobAttackNoAggro
            | Self::Arrow
            | Self::Spit
            | Self::Thrown
            | Self::Thorns
            | Self::Explosion
            | Self::PlayerExplosion
            | Self::Cactus
            | Self::InFire
            | Self::Lava
            | Self::HotFloor
            | Self::SweetBerryBush
            | Self::DryOut => TENTH,
            _ => 0.0,
        }
    }

    /// `#no_anger`：挨打的不记 `lastHurtByMob`。
    ///
    /// `#no_anger`: the victim does not record `lastHurtByMob`.
    pub const fn no_anger(self) -> bool {
        matches!(self, Self::MobAttackNoAggro)
    }

    /// `#panic_environmental_causes`（这里有的那几种；`lightning_bolt` 和 `sulfur_cube_hot` 还没有）。
    ///
    /// `#panic_environmental_causes` (the kinds modelled here; `lightning_bolt` and
    /// `sulfur_cube_hot` do not exist yet).
    pub const fn panic_environmental_causes(self) -> bool {
        matches!(
            self,
            Self::Cactus | Self::Freeze | Self::HotFloor | Self::InFire | Self::Lava | Self::OnFire
        )
    }

    /// `#panic_causes`。没有 `mob_attack_no_aggro`（山羊冲撞不吓人）和 `thorns`。
    ///
    /// `#panic_causes`, which lists neither `mob_attack_no_aggro` (goat rams) nor `thorns`.
    pub const fn panic_causes(self) -> bool {
        matches!(
            self,
            Self::PlayerAttack
                | Self::MobAttack
                | Self::Arrow
                | Self::Thrown
                | Self::Magic
                | Self::IndirectMagic
                | Self::Wither
                | Self::Explosion
                | Self::PlayerExplosion
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
    /// 箭命中后 `AbstractArrow.doKnockback` 用的附魔强度。
    ///
    /// Enchantment strength for the post-hit `AbstractArrow.doKnockback`.
    pub arrow_punch: i32,
    /// 攻击者是驯服的狼时它主人的 UUID：`resolvePlayerResponsibleForDamage` 记到主人头上。
    ///
    /// The owner of a tamed wolf attacker, credited as the responsible player.
    pub owner_credit: Option<[u8; 16]>,
    pub(super) looting_level: i32,
    /// 攻击者的实体类型（注册表 ID）。打人的生物自己正在 tick、不在实体表里，战利品表的
    /// `source_entity` 条件（青蛙吃史莱姆）查不到它，就靠这个。
    ///
    /// The attacker's entity type (registry id). A mob that attacks while ticking is not
    /// in the entity table, so loot `source_entity` conditions (a frog eating a slime)
    /// fall back to this.
    pub attacker_type: Option<i32>,
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
            arrow_punch: 0,
            owner_credit: None,
            looting_level: 0,
            attacker_type: None,
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

    /// 不替代受伤击退：`AbstractArrow.doKnockback` 在成功命中后另加速度。
    ///
    /// Adds to the ordinary damage knockback; this is the separate post-hit arrow push.
    pub fn arrow_push(&self, resistance: f64) -> Option<Vec3> {
        if self.arrow_punch <= 0 {
            return None;
        }
        let movement = self
            .projectile_motion?
            .multiply(1.0, 0.0, 1.0)
            .normalize()
            .scale(f64::from(self.arrow_punch) * 0.6 * (1.0 - resistance).max(0.0));
        (movement.length_sqr() > 0.0).then_some(Vec3::new(movement.x, 0.1, movement.z))
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
            arrow_punch: 0,
            owner_credit: None,
            looting_level: 0,
            attacker_type: None,
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
    /// 羊的 `DATA_WOOL_ID`，战利品表按颜色和是否剪过选子表。
    ///
    /// The sheep `DATA_WOOL_ID`; the loot table picks a sub-table by colour and shear state.
    pub wool: u8,
    /// 史莱姆的大小，战利品表的 `type_specific/cube_mob` 谓词读它。
    ///
    /// The slime size, read by the `type_specific/cube_mob` loot predicate.
    pub cube_size: i32,
    pub source: DamageSource,
    pub recent_player: Option<[u8; 16]>,
    pub recent_player_id: Option<EntityUid>,
    /// 末影人搬着的方块：`dropCustomDeathLoot` 用带精准采集的假工具掉它。
    ///
    /// The block an enderman carries: `dropCustomDeathLoot` drops it with a fake
    /// silk-touch tool.
    pub carried: Option<crate::state::BlockState>,
    /// `Raider.isCaptain()`：是巡逻队长并且头上戴着灾厄旗帜。掠夺者的战利品表按它掉不祥之瓶。
    ///
    /// `Raider.isCaptain()`: a patrol leader wearing the ominous banner. The pillager loot
    /// table drops the ominous bottle on it.
    pub captain: bool,
}

/// 剪毛（`SHEARING`）或下蛋（`GIFT`）的一次战利品表求值：表名、原点、实体本身和可选的工具。
///
/// One shearing (`SHEARING`) or egg-laying (`GIFT`) loot roll: table, origin, the entity itself
/// and the optional tool.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityLootRequest {
    pub table: &'static str,
    pub kind: MobType,
    pub origin: Vec3,
    pub wool: u8,
    pub variant: CowVariant,
    pub tool: Option<mc_bamboo_protocol::item::ItemStack>,
}

impl EntityLootRequest {
    pub(super) fn new(
        table: &'static str,
        me: &super::Entity,
        mob: &Mob,
        tool: Option<mc_bamboo_protocol::item::ItemStack>,
    ) -> Self {
        Self {
            table,
            kind: mob.mob_type,
            origin: me.position,
            // 哞菇借这个字节传品种（0 红 1 棕），羊是羊毛字节。
            // Mooshrooms pass their type here (0 red, 1 brown); sheep pass the wool byte.
            wool: if mob.mob_type == MobType::Mooshroom {
                mob.mooshroom.brown as u8
            } else {
                mob.wool
            },
            variant: mob.variant,
            tool,
        }
    }
}

impl MobDeathLoot {
    pub(super) fn new(me: &super::Entity, mob: &Mob, source: DamageSource) -> Self {
        let credited = mob.last_hurt_by_player_memory_time > 0;
        Self {
            victim: mob.mob_type,
            on_fire: me.remaining_fire_ticks > 0,
            wool: mob.wool,
            cube_size: mob.cube.size,
            source,
            recent_player: mob.last_hurt_by_player_ref.filter(|_| credited),
            recent_player_id: mob.last_hurt_by_player.filter(|_| credited),
            carried: mob.enderman.carried,
            captain: mob.mob_type.is_raider() && raider::is_captain(mob),
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
    /// `Entity.setSharedFlagOnFire` 写进共享标志位的那一位。每刻 `baseTick` 末尾按
    /// `remainingFireTicks > 0` 重算，所以点燃后晚一刻才显示、熄灭当刻就消失。
    ///
    /// The shared-flag bit written by `Entity.setSharedFlagOnFire`. Recomputed at the end
    /// of every `baseTick` from `remainingFireTicks > 0`, so a fresh fire shows one tick
    /// late and an extinguished one clears at once.
    pub visible_fire: bool,
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
    /// `BreakDoorGoal.breakTime`：240 刻破门计时，随实体行搬岛。
    pub door_break_time: i32,
    /// `BreakDoorGoal.lastBreakProgress`：上次发出去的裂纹阶段，构造时 -1，之后不重置。
    pub door_break_progress: i32,
    /// `DoorInteractGoal.doorPos` / `hasDoor` / `passed` / `doorOpenDirX`、`doorOpenDirZ`。
    pub door_pos: BlockPos,
    pub has_door: bool,
    pub door_passed: bool,
    pub door_open_dir: [f32; 2],
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
    pub rabbit: rabbit::Rabbit,
    pub fox: fox::Fox,
    /// `TamableAnimal` 的驯服状态（只有可驯服的种类用）。
    ///
    /// The `TamableAnimal` state, used by tamable kinds only.
    pub tame: tame::Tamable,
    pub wolf: wolf::Wolf,
    pub pig_sound_variant: pig::SoundVariant,
    /// 羊的 `DATA_WOOL_ID`：低 4 位颜色，第 5 位已剪毛。
    ///
    /// The sheep `DATA_WOOL_ID`: colour in the low four bits, bit 4 when sheared.
    pub wool: u8,
    /// 鸡的叫声变种和下蛋倒计时 `eggTime`。
    ///
    /// The chicken sound variant and `eggTime` countdown.
    pub chicken_sound_variant: chicken::SoundVariant,
    pub egg_time: i32,
    /// `Skeleton.freezingTracker`：细雪里转化成流浪者的计时。只有相对计数，随实体行搬岛。
    ///
    /// `Skeleton.freezingTracker`: the powder-snow conversion into a stray. Relative
    /// counters only, so it moves with the entity row.
    pub freeze_conversion: conversion::ConversionTracker,
    /// `Bogged.DATA_SHEARED`：蘑菇已经被剪掉。
    ///
    /// `Bogged.DATA_SHEARED`: the mushrooms have been sheared off.
    pub bogged_sheared: bool,
    /// `Zombie.drowningTracker`：眼睛在水里的刻数和转化倒计时。只有相对计数。
    ///
    /// `Zombie.drowningTracker`: the submerged ticks and the conversion countdown, all
    /// relative.
    pub drown_conversion: conversion::ConversionTracker,
    /// `Entity.isSwimming()`：共享标志第 4 位，水里游泳时不穿水流。
    ///
    /// `Entity.isSwimming()`: shared flag 4; a swimming mob is not pushed by currents.
    pub swimming: bool,
    /// `Drowned.searchingForLand`：`DrownedSwimUpGoal` 期间为真。
    ///
    /// `Drowned.searchingForLand`, set while `DrownedSwimUpGoal` runs.
    pub searching_for_land: bool,
    /// 女巫的喝药状态，见 [`witch::WitchState`]。
    ///
    /// The witch's drinking state, see [`witch::WitchState`].
    pub witch: witch::WitchState,
    /// `Witch.DATA_USING_ITEM`：喝药时为真，单独的同步布尔。
    ///
    /// `Witch.DATA_USING_ITEM`: true while drinking, a separate synced boolean.
    pub witch_drinking: bool,
    /// `PatrollingMonster` / `Raider` 的字段，女巫和掠夺者共用，见 [`raider::Raider`]。
    ///
    /// The `PatrollingMonster` / `Raider` fields shared by witches and pillagers, see
    /// [`raider::Raider`].
    pub raider: raider::Raider,
    /// 弩的使用状态和 `Pillager.IS_CHARGING_CROSSBOW`，见 [`crossbow::CrossbowUse`]。
    ///
    /// The crossbow use state and `Pillager.IS_CHARGING_CROSSBOW`, see
    /// [`crossbow::CrossbowUse`].
    pub crossbow: crossbow::CrossbowUse,
    /// 掠夺者的背包，见 [`pillager::Pillager`]。
    ///
    /// The pillager's inventory, see [`pillager::Pillager`].
    pub pillager: pillager::Pillager,
    /// 史莱姆的大小和移动控制，见 [`slime::CubeState`]。
    ///
    /// The slime size and move control, see [`slime::CubeState`].
    pub cube: slime::CubeState,
    /// 哞菇的品种、存着的迷之炖菜效果和上一道闪电。
    ///
    /// The mooshroom type, its stored stew effects and the last lightning bolt.
    pub mooshroom: mooshroom::Mooshroom,
    /// 北极熊的站立标志和吼叫冷却。
    ///
    /// The polar bear standing flag and growl cooldown.
    pub polar_bear: polar_bear::PolarBear,
    /// 熊猫的基因、旗帜位和计数器。
    ///
    /// The panda's genes, flags and counters.
    pub panda: panda::Panda,
    /// `Entity.isSprinting()`：共享标志第 3 位，同时在 `MOVEMENT_SPEED` 上挂 0.3F 临时加成。
    ///
    /// `Entity.isSprinting()`: shared flag 3, paired with the 0.3F transient speed modifier.
    pub sprinting: bool,
    /// 豹猫的信任标志和潜行姿势。
    ///
    /// The ocelot's trusting flag and crouching pose.
    pub ocelot: ocelot::Ocelot,
    /// 猫的花色、叫声变体、项圈颜色、躺卧和潜行状态。
    ///
    /// The cat's coat/sound variants, collar colour, lying and crouching state.
    pub cat: cat::Cat,
    /// 鹦鹉的品种和扇翅状态。
    ///
    /// The parrot's variant and flapping state.
    pub parrot: parrot::Parrot,
    /// 中立生物的持久愤怒（`NeutralMob`），北极熊在用。
    ///
    /// Persistent anger of neutral mobs (`NeutralMob`); polar bears use it.
    pub anger: neutral::Anger,
    /// 苦力怕的膨胀、引信、带电和点燃状态；别的生物恒为缺省值。全是相对计数，随实体行搬岛。
    ///
    /// Creeper swell, fuse, powered and ignited state; default for other mobs. Relative
    /// counters only, so it moves with the entity row between islands.
    pub creeper: creeper::CreeperState,
    /// 蜘蛛的 `DATA_FLAGS_ID` 第 0 位（`isClimbing`）；别的生物恒为缺省值。
    ///
    /// Bit 0 of the spider's `DATA_FLAGS_ID` (`isClimbing`); default for other mobs.
    pub spider: spider::SpiderState,
    pub(crate) enderman: enderman::EndermanState,
    /// 鱿鱼 / 发光鱿鱼的专属状态，别的生物恒为缺省值。
    ///
    /// Squid and glow squid state; default for every other mob.
    pub squid: squid::SquidState,
    /// 铁傀儡的 `playerCreated` 标志和挥手动画计时。
    pub iron_golem: iron_golem::IronGolem,
    /// 雪傀儡的南瓜标志。
    pub snow_golem: snow_golem::SnowGolem,
    /// 蝙蝠的标志字节和飞行目标；别的生物是缺省值。
    ///
    /// The bat flag byte and flight target; default for every other mob.
    pub bat: bat::BatState,
    /// 幻翼的大小、目标点、锚点、攻击阶段和移动控制的速度；别的生物是缺省值。
    ///
    /// The phantom size, target point, anchor, attack phase and move-control speed;
    /// default for every other mob.
    pub phantom: phantom::PhantomState,
    /// 末影螨的 `Lifetime`；别的生物恒为 0。
    ///
    /// The endermite's `Lifetime`; zero for every other mob.
    pub endermite: endermite::EndermiteState,
    /// 海龟的专属状态，别的生物恒为缺省值。
    ///
    /// Turtle state; default for every other mob.
    pub turtle: turtle::TurtleState,
    /// 鱼的专属状态，别的生物恒为缺省值。
    ///
    /// Fish state; default for every other mob.
    pub fish: fish::FishState,
    /// 海豚的专属状态，别的生物恒为缺省值。
    ///
    /// Dolphin state; default for every other mob.
    pub dolphin: dolphin::DolphinState,
    /// 美西螈的专属状态，别的生物恒为缺省值。
    ///
    /// Axolotl state; default for every other mob.
    pub axolotl: axolotl::AxolotlState,
    /// 这一刻要广播的实体事件（受伤、死亡），游戏层同步时取走。
    pub events: Vec<MobEvent>,
    /// `Entity.needsSync`：这一刻 `jumpFromGround` 跳过，移动包要带上速度。
    ///
    /// `Entity.needsSync`: `jumpFromGround` fired this tick, so the move packets carry velocity.
    pub needs_sync: bool,
    /// 上一次同步之后被击退过（`LivingEntity.knockback` 里的 `needsSync = true`）。受伤常常
    /// 发生在生物自己的一刻之外，所以先记在这里，下一刻开头并进 `needs_sync`。不存档。
    ///
    /// Knocked back since the last sync (`needsSync = true` in `LivingEntity.knockback`).
    /// Hits often land outside the mob's own tick, so the flag waits here and is folded
    /// into `needs_sync` at the start of the next tick. Not saved.
    pub knockback_sync: bool,
    /// `hurtMarked`：速度要强制同步一次。
    pub hurt_marked: bool,
    /// 这一刻 `BreedGoal` 决定生小牛；`living::tick` 在把 `kind` 放回实体之后落地。
    pub pending_breed: Option<EntityUid>,
    /// 骑在它身上的玩家，按上下车顺序。玩家不在实体表里，ID 就是玩家自己的实体 ID；
    /// 官方 `Entity.passengers` 在这里也只会有玩家，因为 Bamboo 还没有生物骑生物。
    ///
    /// The players riding this mob, in boarding order. Players are not in the entity
    /// table, so the id is the player's own entity id. Vanilla `Entity.passengers`
    /// only ever holds players here: Bamboo has no mobs riding mobs yet.
    pub passengers: Vec<EntityUid>,
    /// 这一刻刚被 `ejectPassengers` 甩下来的乘客，供同一刻的 `push_entities` 跳过
    /// （官方甩人时同步把骑手挪到载具旁边，Bamboo 的 `riding::tick` 在实体阶段后才挪）。
    ///
    /// A passenger ejected this tick, skipped by `push_entities` in the same tick
    /// (vanilla moves the rider off the mount synchronously; Bamboo does it in `riding::tick`).
    pub(super) ejected_passenger: Option<EntityUid>,
    /// 这一刻已经处理过几次推挤，以及计数属于哪一刻。`max_entity_collisions`
    /// 打开时才有用；刻号不同就把计数当作 0，不用每刻扫表清零。
    /// 不存档、不同步。
    ///
    /// Pushes handled this tick and the tick the count belongs to. A count
    /// from an older tick reads as zero, so no per-tick store scan is needed.
    /// Not saved or synced.
    pub(super) push_count: (i64, usize),
    pub equine: Option<Box<equine::EquineState>>,
    /// 跑大脑的生物（犰狳）的 `Brain`；用目标选择器的生物是 `None`。
    ///
    /// The `Brain` of brain-driven mobs (armadillos); `None` for goal-driven mobs.
    pub brain: Option<Box<brain::Brain>>,
    /// 犰狳的状态机和掉鳞甲计时。
    ///
    /// The armadillo state machine and scute timer.
    pub armadillo: armadillo::Armadillo,
    /// 山羊的同步数据和长跳姿势。
    ///
    /// The goat synced data and long-jump pose.
    pub goat: goat::Goat,
    /// 青蛙的品种、姿势和伸舌头的目标。
    ///
    /// The frog's variant, pose and tongue target.
    pub frog: frog::Frog,
    /// 骆驼的姿势状态机同步值和冲刺计时。
    ///
    /// The camel's pose state machine synced values and dash timer.
    pub camel: camel::Camel,
    /// 蜜蜂的元数据字节、巢 / 花位置和授粉状态；别的生物是缺省值。
    pub bee: bee::Bee,
    /// `Tadpole.age`：到 24000 变成青蛙。蝌蚪不是 `AgeableMob`，和 `age` 是两回事。
    ///
    /// `Tadpole.age`: a frog at 24000. Tadpoles are not `AgeableMob`s, so this is
    /// separate from `age`.
    pub tadpole_age: i32,
    /// `LivingEntity.discardFriction`：长跳途中 `travelInAir` 不乘摩擦。
    ///
    /// `LivingEntity.discardFriction`: `travelInAir` skips friction during a long jump.
    pub discard_friction: bool,
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
    /// `ServerLevel.sendParticles`：一个粒子包，发给 32 格内的玩家（按玩家所在方块的中心算）。
    ///
    /// `ServerLevel.sendParticles`: one particle packet for players within 32 blocks,
    /// measured from the centre of the player's block.
    Particles {
        particle: i32,
        position: Vec3,
        offset: Vec3,
        speed: f32,
        count: i32,
    },
    /// 玩家音源（`SoundSource.PLAYERS`）的音效，比如给羊染色。
    ///
    /// A sound in the player category, such as dyeing a sheep.
    PlayerSound { sound: &'static str, position: Vec3 },
    /// 同上，但带音量：弩的装填音效是 `SoundSource.PLAYERS`、音量 0.5。
    ///
    /// As above with a volume: crossbow loading sounds use `SoundSource.PLAYERS` at 0.5.
    PlayerSoundAt {
        sound: &'static str,
        position: Vec3,
        volume: f32,
    },
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
        seed: Option<i64>,
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
            MobType::Cow | MobType::Mooshroom => Attributes::cow(),
            MobType::Rabbit => rabbit::attributes(),
            MobType::Fox => fox::attributes(),
            MobType::Wolf => wolf::attributes(),
            MobType::Pig => pig::attributes(),
            MobType::Sheep => sheep::attributes(),
            MobType::Chicken => chicken::attributes(),
            MobType::Skeleton | MobType::Stray => Attributes::skeleton(),
            MobType::Bogged => bogged::attributes(),
            MobType::Slime => slime::attributes(),
            MobType::PolarBear => polar_bear::attributes(),
            MobType::Panda => panda::attributes(),
            MobType::Ocelot => ocelot::attributes(),
            MobType::Cat => cat::attributes(),
            MobType::Parrot => parrot::attributes(),
            MobType::Squid | MobType::GlowSquid => squid::attributes(),
            MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish => {
                fish::attributes()
            }
            MobType::Turtle => turtle::attributes(),
            MobType::Dolphin => dolphin::attributes(),
            MobType::Axolotl => axolotl::attributes(),
            MobType::Creeper => creeper::attributes(),
            MobType::Enderman => enderman::attributes(),
            MobType::Spider => spider::attributes(MobType::Spider),
            MobType::CaveSpider => spider::attributes(MobType::CaveSpider),
            MobType::Zombie | MobType::Husk => Attributes::zombie(),
            MobType::Drowned => drowned::attributes(),
            MobType::Witch => witch::attributes(),
            MobType::Pillager => pillager::attributes(),
            MobType::Armadillo => armadillo::attributes(),
            MobType::Goat => goat::attributes(),
            MobType::Frog => frog::attributes(),
            MobType::Tadpole => tadpole::attributes(),
            MobType::Bat => bat::attributes(),
            MobType::Phantom => phantom::attributes(),
            MobType::Camel => camel::attributes(),
            MobType::Bee => bee::attributes(),
            MobType::IronGolem => iron_golem::attributes(),
            MobType::SnowGolem => snow_golem::attributes(),
            MobType::Silverfish => silverfish::attributes(),
            MobType::Endermite => endermite::attributes(),
            kind => equine::attributes(kind),
        };
        let follow_range = attributes.base(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
        let mut navigation = Navigation::new(follow_range);
        if mob_type == MobType::Drowned {
            // `Drowned.createNavigation` 给的是 `AmphibiousPathNavigation`：恒可重算路径、
            // 水里也走、水里贴墙算水边，陆地和水边更贵（代价在 `Mob::malus`）。
            //
            // `Drowned.createNavigation` builds an `AmphibiousPathNavigation`: paths may
            // always be recomputed, it walks in water, water against a wall is a water
            // border, and land costs more (the maluses live in `Mob::malus`).
            navigation.flags.amphibious = true;
        }
        if mob_type == MobType::Llama {
            // `Llama` 构造函数：`this.getNavigation().setRequiredPathLength(40.0F)`。
            // `Llama` constructor: `this.getNavigation().setRequiredPathLength(40.0F)`.
            navigation.required_path_length = 40.0;
            navigation.update_max_visited_nodes(follow_range);
        }
        if mob_type.climbs_walls() {
            // `Spider.createNavigation` 用 `WallClimberNavigation`。
            // `Spider.createNavigation` builds a `WallClimberNavigation`.
            navigation.wall_climber = true;
        }
        let goals = match mob_type {
            MobType::Cow | MobType::Mooshroom => GoalSelector::cow(&mut navigation),
            MobType::Rabbit => rabbit::goals(&mut navigation),
            MobType::Fox => fox::goals(&mut navigation, 0),
            MobType::Wolf => wolf::goals(&mut navigation),
            MobType::Pig => pig::goals(&mut navigation),
            MobType::Sheep => sheep::goals(&mut navigation),
            MobType::Chicken => chicken::goals(&mut navigation),
            MobType::Skeleton | MobType::Stray | MobType::Bogged => GoalSelector::skeleton(),
            MobType::Slime => slime::goals(&mut navigation),
            MobType::PolarBear => polar_bear::goals(&mut navigation),
            MobType::Panda => panda::goals(&mut navigation),
            MobType::Ocelot => ocelot::goals(&mut navigation),
            MobType::Cat => cat::goals(&mut navigation),
            MobType::Parrot => parrot::goals(&mut navigation),
            MobType::Squid | MobType::GlowSquid => squid::goals(),
            MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish => {
                fish::goals(mob_type)
            }
            MobType::Turtle => turtle::goals(&mut navigation),
            MobType::Dolphin => dolphin::goals(),
            MobType::Axolotl => axolotl::goals(),
            MobType::Creeper => creeper::goals(&mut navigation),
            MobType::Enderman => enderman::goals(&mut navigation),
            MobType::Spider | MobType::CaveSpider => spider::goals(&mut navigation),
            MobType::Zombie | MobType::Husk => GoalSelector::zombie(&mut navigation),
            MobType::Drowned => drowned::goals(&mut navigation),
            MobType::Witch => witch::goals(&mut navigation),
            MobType::Pillager => pillager::goals(&mut navigation),
            // 蝙蝠和骆驼没有覆写 `registerGoals`：两个选择器都是空的（骆驼不要 `equine::goals`）。
            // Bats and camels do not override `registerGoals`: both selectors stay empty
            // (no `equine::goals` for camels).
            MobType::Armadillo | MobType::Goat | MobType::Frog | MobType::Bat | MobType::Camel => {
                GoalSelector::default()
            }
            // 蝌蚪没有覆盖 `AbstractFish.registerGoals`，鱼的目标和大脑一起跑。
            // Tadpoles do not override `AbstractFish.registerGoals`: the fish goals run
            // alongside the brain.
            MobType::Tadpole => fish::goals(mob_type),
            MobType::Phantom => phantom::goals(),
            MobType::Bee => bee_goals::goals(&mut navigation),
            MobType::IronGolem => iron_golem::goals(&mut navigation),
            MobType::SnowGolem => snow_golem::goals(&mut navigation),
            MobType::Silverfish => silverfish::goals(&mut navigation),
            MobType::Endermite => endermite::goals(&mut navigation),
            kind => equine::goals(kind, &mut navigation),
        };
        if fish::is_fish(mob_type) {
            // `WaterBoundPathNavigation.createPathFinder`：只有海豚允许跃出水面。
            // `WaterBoundPathNavigation.createPathFinder`: only dolphins may breach.
            navigation.flags.swim = Some(false);
        }
        if mob_type == MobType::Dolphin {
            // `WaterBoundPathNavigation(this, level)`，`allowBreaching = true`。
            // `WaterBoundPathNavigation(this, level)` with allowBreaching = true.
            navigation.flags.swim = Some(true);
        }
        if mob_type == MobType::Axolotl {
            // `AmphibiousPathNavigation(this, level)`。
            navigation.flags.amphibious = true;
        }
        let target_goals = match mob_type {
            MobType::Cow
            | MobType::Mooshroom
            | MobType::Pig
            | MobType::Sheep
            | MobType::Chicken
            | MobType::Armadillo
            | MobType::Goat
            | MobType::Frog
            | MobType::Tadpole
            | MobType::Bat
            | MobType::Camel => GoalSelector::default(),
            MobType::PolarBear => polar_bear::target_goals(),
            MobType::Panda => panda::target_goals(),
            MobType::Ocelot => ocelot::target_goals(),
            MobType::Cat => cat::target_goals(),
            MobType::Parrot => GoalSelector::default(),
            MobType::Fox => fox::target_goals(),
            MobType::Wolf => wolf::target_goals(),
            MobType::Rabbit
            | MobType::Squid
            | MobType::GlowSquid
            | MobType::Cod
            | MobType::Salmon
            | MobType::TropicalFish
            | MobType::Pufferfish
            | MobType::Axolotl => GoalSelector::default(),
            MobType::Dolphin => dolphin::target_goals(),
            MobType::Skeleton | MobType::Stray | MobType::Bogged => {
                GoalSelector::skeleton_targets()
            }
            MobType::Creeper => creeper::target_goals(),
            MobType::Enderman => enderman::target_goals(),
            MobType::Spider | MobType::CaveSpider => spider::target_goals(&mut navigation),
            MobType::Zombie | MobType::Husk => GoalSelector::zombie_targets(),
            MobType::Slime => slime::targets(),
            MobType::Drowned => drowned::targets(),
            MobType::Witch => witch::targets(),
            MobType::Phantom => phantom::target_goals(),
            MobType::Bee => bee_goals::target_goals(),
            MobType::Pillager => pillager::targets(),
            MobType::IronGolem => iron_golem::target_goals(),
            MobType::SnowGolem => snow_golem::target_goals(),
            MobType::Silverfish => silverfish::targets(),
            MobType::Endermite => endermite::targets(),
            kind => equine::targets(kind),
        };
        let health = attributes.value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32;
        let mut mob = Self {
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
            air_supply: mob_type.max_air_supply(),
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
            visible_fire: false,
            ambient_sound_time: 0,
            look: LookControl::default(),
            mover: if mob_type == MobType::Rabbit {
                rabbit::initial_move_control()
            } else {
                MoveControl::default()
            },
            jump: JumpControl::default(),
            body: BodyRotationControl::default(),
            navigation,
            goals,
            seen: Vec::new(),
            malus: match mob_type {
                MobType::Drowned => {
                    // 官方 `Drowned` 构造函数只把水的代价设成 0；陆地和浅水边缘的
                    // 6 / 4 是 `AmphibiousNodeEvaluator.start()` 在搜索期间临时改的，
                    // 搜索结束就还原，不能常驻在生物身上（`GoalUtils.hasMalus` 会读它）。
                    //
                    // Vanilla `Drowned` only zeroes the water cost. The 6 / 4 land and
                    // water-border costs are set temporarily by
                    // `AmphibiousNodeEvaluator.start()` for a search and restored on done.
                    let mut malus = Malus::monster();
                    malus.set(mc_bamboo_data::PathType::Water, 0.0);
                    malus
                }
                _ if mob_type.is_monster() => Malus::monster(),
                MobType::Chicken => chicken::malus(),
                MobType::Enderman => enderman::malus(),
                // `AgeableWaterCreature` 构造函数：`setPathfindingMalus(WATER, 0)`。
                // The `AgeableWaterCreature` constructor zeroes the water malus.
                MobType::Squid | MobType::GlowSquid => squid::malus(),
                MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish => {
                    fish::malus()
                }
                MobType::Turtle => turtle::malus(),
                MobType::Dolphin => dolphin::malus(),
                MobType::Axolotl => axolotl::malus(),
                MobType::Tadpole => fish::malus(),
                // 蝙蝠直接继承 `Mob`，没有 `Animal` 的两项覆盖。
                // Bats extend `Mob` directly, without the two `Animal` overrides.
                MobType::Bat | MobType::Phantom => Malus::monster(),
                _ => Malus::animal(),
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
            door_pos: BlockPos::new(0, 0, 0),
            has_door: false,
            door_passed: false,
            door_open_dir: [0.0; 2],
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
            rabbit: rabbit::Rabbit::default(),
            fox: fox::Fox::default(),
            tame: tame::Tamable::default(),
            wolf: wolf::Wolf::default(),
            pig_sound_variant: pig::SoundVariant::Classic,
            wool: 0,
            chicken_sound_variant: chicken::SoundVariant::Classic,
            egg_time: 0,
            freeze_conversion: conversion::ConversionTracker::default(),
            bogged_sheared: false,
            drown_conversion: conversion::ConversionTracker::default(),
            swimming: false,
            searching_for_land: false,
            witch: witch::WitchState::default(),
            witch_drinking: false,
            raider: raider::Raider::default(),
            crossbow: crossbow::CrossbowUse::default(),
            pillager: pillager::Pillager::default(),
            cube: slime::CubeState::default(),
            mooshroom: mooshroom::Mooshroom::default(),
            polar_bear: polar_bear::PolarBear::default(),
            panda: panda::Panda::default(),
            sprinting: false,
            ocelot: ocelot::Ocelot::default(),
            cat: cat::Cat::default(),
            parrot: parrot::Parrot::default(),
            anger: neutral::Anger::default(),
            creeper: creeper::CreeperState::default(),
            spider: spider::SpiderState::default(),
            enderman: enderman::EndermanState::default(),
            squid: squid::SquidState::default(),
            // `Bat` 构造函数：服务端一出生就倒挂。
            // The `Bat` constructor: a server-side bat starts out resting.
            bat: bat::BatState {
                flags: u8::from(mob_type == MobType::Bat),
                target: None,
            },
            phantom: phantom::PhantomState::default(),
            endermite: endermite::EndermiteState::default(),
            turtle: turtle::TurtleState::default(),
            fish: fish::FishState::new(),
            dolphin: dolphin::DolphinState::new(),
            axolotl: axolotl::AxolotlState::new(),
            events: Vec::new(),
            needs_sync: false,
            knockback_sync: false,
            hurt_marked: false,
            pending_breed: None,
            brain: new_brain(mob_type),
            armadillo: armadillo::Armadillo::default(),
            goat: goat::Goat::default(),
            frog: frog::Frog::default(),
            camel: camel::Camel::default(),
            bee: bee::Bee::default(),
            iron_golem: iron_golem::IronGolem::default(),
            snow_golem: snow_golem::SnowGolem::default(),
            tadpole_age: 0,
            discard_friction: false,
            passengers: Vec::new(),
            ejected_passenger: None,
            push_count: (i64::MIN, 0),
            equine: mob_type
                .is_equine()
                .then(|| Box::new(equine::EquineState::default())),
        };
        if mob_type == MobType::Salmon {
            // `Salmon` 的 `DATA_TYPE` 缺省是 `Variant.MEDIUM`。
            // The `Salmon` `DATA_TYPE` default is `Variant.MEDIUM`.
            mob.fish.variant = 1;
        }
        if mob_type == MobType::Fox {
            fox::configure(&mut mob);
        }
        if mob_type == MobType::Goat {
            // `Goat` 构造函数：能浮水，细雪和细雪上方都不走。
            // The `Goat` constructor: floats, and powder snow is off limits.
            mob.navigation.flags.can_float = true;
            mob.malus.set(mc_bamboo_data::PathType::PowderSnow, -1.0);
            mob.malus
                .set(mc_bamboo_data::PathType::OnTopOfPowderSnow, -1.0);
        }
        if mob_type == MobType::Frog {
            frog::configure(&mut mob);
        }
        if mob_type == MobType::Tadpole {
            // `Tadpole.fromBucket()` 恒真（`setFromBucket` 是空的）：永远不会自然消失。
            // `Tadpole.fromBucket()` is always true (`setFromBucket` does nothing), so
            // tadpoles never despawn.
            mob.fish.from_bucket = true;
        }
        if mob_type == MobType::Armadillo {
            // `Armadillo` 构造函数里的 `getNavigation().setCanFloat(true)`。
            // The `Armadillo` constructor's `getNavigation().setCanFloat(true)`.
            mob.navigation.flags.can_float = true;
        }
        if mob_type == MobType::Camel {
            camel::configure(&mut mob);
        }
        if mob_type == MobType::Parrot {
            parrot::configure(&mut mob);
        }
        if mob_type == MobType::Bee {
            bee::configure(&mut mob);
        }
        if mob_type == MobType::Wolf {
            wolf::configure(&mut mob);
        }
        mob
    }

    /// `PowderSnowBlock.canEntityWalkOnPowderSnow`：类型在 `powder_snow_walkable_mobs`
    /// 里，或者脚上穿着皮革靴。
    ///
    /// `PowderSnowBlock.canEntityWalkOnPowderSnow`: the type is in
    /// `powder_snow_walkable_mobs` or the feet slot holds leather boots.
    pub fn walks_on_powder_snow(&self) -> bool {
        self.mob_type
            .in_entity_tag("minecraft:powder_snow_walkable_mobs")
            || self
                .equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Feet)
                .item
                == mc_bamboo_data::items::LEATHER_BOOTS
    }

    /// `Mob.getPathfindingMalus`。两栖寻路跑过一次以后水的代价是 0：
    /// `AmphibiousNodeEvaluator.prepare` 改了它，`done` 没有改回来。
    ///
    /// `Mob.getPathfindingMalus`. Water costs 0 once an amphibious search has run:
    /// `AmphibiousNodeEvaluator.prepare` changes it and `done` never restores it.
    pub fn pathfinding_malus(&self, path_type: mc_bamboo_data::PathType) -> f32 {
        if path_type == mc_bamboo_data::PathType::Water && self.navigation.cleared_water_malus {
            0.0
        } else {
            self.malus.get(path_type)
        }
    }

    pub fn is_baby(&self) -> bool {
        if self.mob_type.uses_baby_metadata() {
            self.zombie_baby
        } else {
            // `AgeableMob.isBaby`：`canBeABaby() && getAge() < 0`。
            self.mob_type.can_be_a_baby() && self.age < 0
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
        if self.mob_type == MobType::Slime {
            let (width, height, _) = slime::dimensions(self.cube.size);
            return (width, height);
        }
        if self.mob_type == MobType::Phantom {
            let (width, height, _) = phantom::dimensions(self.phantom.size);
            return (width, height);
        }
        if self.mob_type == MobType::Goat {
            let (width, height, _) = goat::pose_dimensions(self.is_baby(), self.goat.long_jumping);
            return (width, height);
        }
        if self.mob_type == MobType::Camel {
            let sitting = self.camel.pose == camel::Pose::Sitting;
            let (width, height, _) = camel::dimensions(self.is_baby(), sitting);
            return (width, height);
        }
        if fish::is_fish(self.mob_type) {
            let (width, height, _) =
                fish::dimensions(self.mob_type, self.fish.variant, self.fish.puff_state);
            return (width, height);
        }
        let (width, height, _) = self.mob_type.dimensions(self.is_baby());
        (width, height)
    }

    /// `Mob.removeWhenFarAway`。鱼覆盖成 `!fromBucket && !hasCustomName`；
    /// 豹猫覆盖成 `!isTrusting && tickCount > 2400`。
    ///
    /// `Mob.removeWhenFarAway`; fish override it with `!fromBucket && !hasCustomName`,
    /// and ocelots with `!isTrusting && tickCount > 2400`.
    pub fn removes_when_far_away(&self, tick_count: i32) -> bool {
        self.removes_when_far_away_at(f64::INFINITY, tick_count)
    }

    /// `Mob.removeWhenFarAway(distSqr)`：`PatrollingMonster` 的覆写要看距离——巡逻中的
    /// 只有离最近玩家超过 128 格才会消失。
    ///
    /// `Mob.removeWhenFarAway(distSqr)`: the `PatrollingMonster` override reads the
    /// distance — a patrolling mob only despawns beyond 128 blocks from the nearest player.
    pub fn removes_when_far_away_at(&self, distance_sqr: f64, tick_count: i32) -> bool {
        if self.mob_type.is_raider() {
            return raider::removes_when_far_away(self, distance_sqr);
        }
        if fish::is_fish(self.mob_type) {
            return !self.fish.from_bucket && self.custom_name.is_none();
        }
        if self.mob_type == MobType::Axolotl {
            // `Axolotl.removeWhenFarAway`：装过桶或起了名就不消失。
            // `Axolotl.removeWhenFarAway`: never once bucketed or named.
            return !self.axolotl.from_bucket && self.custom_name.is_none();
        }
        if self.mob_type == MobType::Ocelot {
            return ocelot::removes_when_far_away(self, tick_count);
        }
        if self.mob_type == MobType::Cat {
            return cat::removes_when_far_away(self, tick_count);
        }
        self.mob_type.removes_when_far_away()
    }

    /// `isPersistenceRequired() || requiresCustomPersistence()`：从桶里放出来的鱼
    /// （`AbstractFish`）、装过桶的美西螈和搬着方块的末影人（`Enderman`）不会自然消失。
    ///
    /// `isPersistenceRequired() || requiresCustomPersistence()`: fish released from a
    /// bucket (`AbstractFish`), bucketed axolotls and endermen carrying a block
    /// (`Enderman`) never despawn.
    pub fn requires_custom_persistence(&self) -> bool {
        self.persistence_required
            || (fish::is_fish(self.mob_type) && self.fish.from_bucket)
            || (self.mob_type == MobType::Axolotl && self.axolotl.from_bucket)
            || self.enderman.carried.is_some()
    }

    /// `isDeadOrDying`。
    pub fn is_dead_or_dying(&self) -> bool {
        self.health <= 0.0 || self.dead
    }

    /// `Entity.isVehicle()`：有人骑着。
    ///
    /// `Entity.isVehicle()`: someone is riding.
    pub fn is_vehicle(&self) -> bool {
        !self.passengers.is_empty()
    }

    /// `Entity.getFirstPassenger()`：最先上车的那个。
    ///
    /// `Entity.getFirstPassenger()`: whoever boarded first.
    pub fn first_passenger(&self) -> Option<EntityUid> {
        self.passengers.first().copied()
    }

    /// `Entity.getControllingPassenger()`：上鞍的载具由第一位乘客控制。
    /// 官方的马、猪和骆驼都是这一条；羊驼没有鞍位，所以永远不由玩家控制。
    ///
    /// `Entity.getControllingPassenger()`: a saddled vehicle is controlled by its
    /// first passenger. Vanilla horses, pigs and camels follow this rule; llamas
    /// have no saddle slot, so a player never controls one.
    pub fn controlling_passenger(&self) -> Option<EntityUid> {
        if self
            .equipment
            .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Saddle)
            .is_empty()
        {
            return None;
        }
        self.first_passenger()
    }

    /// `PathfinderMob.isPanicking`：大脑里有 `IS_PANICKING` 记忆，或者正在跑的目标里有恐慌。
    ///
    /// `PathfinderMob.isPanicking`: an `IS_PANICKING` memory in the brain, or a running
    /// panic goal.
    pub fn is_panicking(&self) -> bool {
        if let Some(brain) = &self.brain
            && brain.memories.has(brain::memory::MemoryType::IsPanicking)
        {
            return true;
        }
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
        self.anger.rebase(delta);
        self.wolf.rebase(delta);
    }
}

/// 别的实体和追踪同步能读到的生物数据。
#[derive(Debug, Clone, PartialEq)]
pub struct MobPublic {
    /// `Bee.DATA_FLAGS_ID` 和 `DATA_ANGER_END_TIME`。
    pub bee_flags: u8,
    pub bee_anger_end_time: i64,
    pub mob_type: MobType,
    pub uuid: [u8; 16],
    /// 同步 `update_attributes` 用的属性表（只含已经实例化的条目）。
    ///
    /// The attribute table for `update_attributes` (instantiated entries only).
    pub attributes: Attributes,
    pub health: f32,
    /// 当前挂着的状态效果，同步 `DATA_EFFECT_PARTICLES` / `DATA_EFFECT_AMBIENCE_ID` 用。
    ///
    /// The active status effects, synced through `DATA_EFFECT_PARTICLES` /
    /// `DATA_EFFECT_AMBIENCE_ID`.
    pub status_effects: effects::EffectMap,
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
    pub rabbit_variant: i32,
    /// `Fox.DATA_TYPE_ID` / `DATA_FLAGS_ID` / 两个信任格。
    ///
    /// The fox variant, flags and two trusted slots.
    pub fox_variant: i32,
    pub fox_flags: u8,
    pub fox_trusted: [Option<[u8; 16]>; 2],
    /// `TamableAnimal.DATA_FLAGS_ID` 和主人。
    ///
    /// The tamable flags and owner.
    pub tame_flags: u8,
    pub owner: Option<[u8; 16]>,
    /// 原始的 `target` 字段有没有值（`HurtByTargetGoal.alertOthers` 看它）。
    ///
    /// Whether the raw target is set; read when alerting others.
    pub has_target: bool,
    /// 狼的元数据：讨食、项圈、仇恨结束时刻、品种、叫声。
    ///
    /// The wolf metadata: interested, collar, anger end, variant, sound variant.
    pub wolf_interested: bool,
    pub wolf_collar: i32,
    pub wolf_anger_end_time: i64,
    pub wolf_variant: i32,
    pub wolf_sound_variant: i32,
    pub needs_sync: bool,
    pub pig_sound_variant: pig::SoundVariant,
    pub wool: u8,
    pub chicken_sound_variant: chicken::SoundVariant,
    /// `Armadillo.ARMADILLO_STATE` 的网络 ID。
    ///
    /// The network id of `Armadillo.ARMADILLO_STATE`.
    pub armadillo_state: i32,
    /// 山羊的三个同步布尔值和长跳姿势。
    ///
    /// The goat synced booleans and its long-jump pose.
    pub goat: goat::Goat,
    /// `MushroomCow.DATA_TYPE`：棕色为真。
    ///
    /// `MushroomCow.DATA_TYPE`: true for brown.
    pub mooshroom_brown: bool,
    /// `PolarBear.DATA_STANDING_ID`。
    ///
    /// `PolarBear.DATA_STANDING_ID`.
    pub polar_bear_standing: bool,
    /// 熊猫的同步字段。
    ///
    /// The synced panda fields.
    pub panda: panda::PandaPublic,
    /// `Entity.isSprinting()`：共享标志第 3 位。
    ///
    /// `Entity.isSprinting()`: shared flag 3.
    pub sprinting: bool,
    /// `Ocelot.DATA_TRUSTING` 与潜行姿势。
    ///
    /// `Ocelot.DATA_TRUSTING` and the crouching pose.
    pub ocelot_trusting: bool,
    pub ocelot_crouching: bool,
    /// 猫的同步状态。
    ///
    /// Synced state of a cat.
    pub cat: cat::Cat,
    /// 鹦鹉的同步状态。
    ///
    /// Synced state of a parrot.
    pub parrot: parrot::Parrot,
    /// 苦力怕的三项同步元数据：`DATA_SWELL_DIR`、`DATA_IS_POWERED`、`DATA_IS_IGNITED`。
    ///
    /// The three synced creeper items: swell direction, powered and ignited.
    pub creeper_swell_dir: i32,
    pub creeper_powered: bool,
    pub creeper_ignited: bool,
    /// 蜘蛛的 `DATA_FLAGS_ID`，攀爬位。
    ///
    /// The spider `DATA_FLAGS_ID`, with the climbing bit.
    pub spider_flags: u8,
    pub enderman_carried: Option<crate::state::BlockState>,
    pub enderman_creepy: bool,
    pub enderman_stared_at: bool,
    /// `GlowSquid.DATA_DARK_TICKS_REMAINING`。
    pub dark_ticks: i32,
    /// `IronGolem.DATA_FLAGS_ID` / `SnowGolem.DATA_PUMPKIN_ID`，都是 BYTE。
    pub golem_flags: u8,
    /// `Bat.DATA_ID_FLAGS`。
    pub bat_flags: u8,
    /// `Phantom.ID_SIZE`。
    pub phantom_size: i32,
    /// 鱼的桶标记、花纹和河豚的膨胀程度。
    ///
    /// The fish bucket flag, variant and the pufferfish puff state.
    pub fish_from_bucket: bool,
    pub fish_variant: i32,
    pub puff_state: i32,
    /// `AbstractSchoolingFish.leader` 的 uid。
    ///
    /// The uid of `AbstractSchoolingFish.leader`.
    pub fish_leader: Option<EntityUid>,
    /// `AbstractSchoolingFish.schoolSize`。
    ///
    /// `AbstractSchoolingFish.schoolSize`.
    pub school_size: i32,
    /// 青蛙的品种、姿势和舌头目标。
    ///
    /// The frog's variant, pose and tongue target.
    pub frog: frog::Frog,
    /// 骆驼的 `DASH` / `LAST_POSE_CHANGE_TICK` / 姿势。
    ///
    /// The camel's `DASH` / `LAST_POSE_CHANGE_TICK` / pose.
    pub camel: camel::Camel,
    /// `Turtle.HAS_EGG` / `LAYING_EGG`。
    ///
    /// `Turtle.HAS_EGG` / `LAYING_EGG`.
    pub turtle_has_egg: bool,
    pub turtle_laying_egg: bool,
    /// `Dolphin.GOT_FISH` / `MOISTNESS_LEVEL`。
    ///
    /// `Dolphin.GOT_FISH` / `MOISTNESS_LEVEL`.
    pub dolphin_got_fish: bool,
    pub dolphin_moistness: i32,
    /// `Axolotl.DATA_VARIANT` / `DATA_PLAYING_DEAD` / `FROM_BUCKET`。
    ///
    /// `Axolotl.DATA_VARIANT` / `DATA_PLAYING_DEAD` / `FROM_BUCKET`.
    pub axolotl_variant: i32,
    pub axolotl_playing_dead: bool,
    pub axolotl_from_bucket: bool,
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
    /// `Witch.DATA_USING_ITEM`。
    ///
    /// `Witch.DATA_USING_ITEM`.
    pub witch_using_item: bool,
    /// `Pillager.IS_CHARGING_CROSSBOW`。
    pub pillager_charging: bool,
    /// 目标药水效果标志（缓慢 / 中毒 / 虚弱），女巫选药时读它。
    ///
    /// Target potion effect flags (slowness, poison, weakness), read by witches.
    pub effects: super::ctx::TargetEffects,
    /// `Entity.isSwimming()`：共享标志第 4 位。
    ///
    /// `Entity.isSwimming()`: shared flag 4.
    pub swimming: bool,
    /// `Entity.isInWater()`：上一刻的流体跟踪结果，别的生物判断目标是否在水里要用。
    ///
    /// `Entity.isInWater()` from the last fluid update; other mobs read it to decide
    /// whether a target is in water.
    pub in_water: bool,
    /// 骷髅一支在 `Mob` 之后的第一个布尔元数据：骷髅是 `DATA_STRAY_CONVERSION_ID`，
    /// 沼骸是 `DATA_SHEARED`，流浪者没有。
    ///
    /// The first boolean after `Mob` for the skeleton family: `DATA_STRAY_CONVERSION_ID`
    /// on skeletons, `DATA_SHEARED` on bogged, absent on strays.
    pub skeleton_flag: bool,
    /// `AbstractCubeMob.ID_SIZE`；不是史莱姆时恒为 1。
    ///
    /// `AbstractCubeMob.ID_SIZE`; always 1 for other mobs.
    pub cube_size: i32,
    /// `Mob.isPersistenceRequired()`：持久化的生物不参与自然刷怪的计数。
    ///
    /// `Mob.isPersistenceRequired()`: persistent mobs are left out of the spawn counts.
    pub persistence_required: bool,
    /// 非空的装备槽，按 ordinal；配对时的 `set_equipment` 用它。
    pub equipment: Vec<(
        mc_bamboo_protocol::packet::play::EquipmentSlot,
        mc_bamboo_protocol::item::ItemStack,
    )>,
    /// 骑在上面的玩家，同步阶段按它决定要不要发 `set_passengers`。
    ///
    /// The players riding this mob; the sync layer sends `set_passengers` from it.
    pub passengers: Vec<EntityUid>,
}

impl MobPublic {
    pub fn of(mob: &Mob, in_water: bool) -> Self {
        Self {
            bee_flags: mob.bee.flags,
            bee_anger_end_time: mob.bee.anger_end_time,
            mob_type: mob.mob_type,
            uuid: mob.uuid,
            attributes: mob.attributes.clone(),
            health: mob.health,
            status_effects: mob.effects.clone(),
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
            rabbit_variant: mob.rabbit.variant.id(),
            fox_variant: mob.fox.variant.id(),
            fox_flags: mob.fox.flags,
            fox_trusted: mob.fox.trusted,
            tame_flags: mob.tame.flags,
            owner: mob.tame.owner,
            has_target: mob.target.is_some(),
            wolf_interested: mob.wolf.interested,
            wolf_collar: mob.wolf.collar,
            wolf_anger_end_time: mob.wolf.anger_end_time,
            wolf_variant: mob.wolf.variant,
            wolf_sound_variant: mob.wolf.sound_variant,
            needs_sync: mob.needs_sync,
            pig_sound_variant: mob.pig_sound_variant,
            wool: mob.wool,
            chicken_sound_variant: mob.chicken_sound_variant,
            armadillo_state: mob.armadillo.state.id(),
            goat: mob.goat,
            mooshroom_brown: mob.mooshroom.brown,
            polar_bear_standing: mob.polar_bear.standing,
            panda: panda::PandaPublic::of(&mob.panda),
            sprinting: mob.sprinting,
            ocelot_trusting: mob.ocelot.trusting,
            ocelot_crouching: mob.ocelot.crouching,
            cat: mob.cat,
            parrot: mob.parrot.clone(),
            creeper_swell_dir: mob.creeper.swell_dir,
            creeper_powered: mob.creeper.powered,
            creeper_ignited: mob.creeper.ignited,
            spider_flags: if mob.spider.climbing { 1 } else { 0 },
            enderman_carried: mob.enderman.carried,
            enderman_creepy: mob.enderman.creepy,
            enderman_stared_at: mob.enderman.stared_at,
            dark_ticks: mob.squid.dark_ticks,
            bat_flags: mob.bat.flags,
            golem_flags: if mob.mob_type == MobType::IronGolem {
                mob.iron_golem.flags
            } else {
                mob.snow_golem.flags
            },
            phantom_size: mob.phantom.size,
            frog: mob.frog,
            camel: mob.camel,
            // 蝌蚪的 `FROM_BUCKET` 同步数据从来不写，一直是假。
            // A tadpole never writes its synced `FROM_BUCKET`, which stays false.
            fish_from_bucket: mob.fish.from_bucket && mob.mob_type != MobType::Tadpole,
            fish_variant: mob.fish.variant,
            puff_state: mob.fish.puff_state,
            fish_leader: mob.fish.leader,
            school_size: mob.fish.school_size,
            turtle_has_egg: mob.turtle.has_egg,
            turtle_laying_egg: mob.turtle.laying_egg,
            dolphin_got_fish: mob.dolphin.got_fish,
            dolphin_moistness: mob.dolphin.moistness,
            axolotl_variant: mob.axolotl.variant,
            axolotl_playing_dead: mob.axolotl.playing_dead,
            axolotl_from_bucket: mob.axolotl.from_bucket,
            on_fire: mob.visible_fire,
            persistence_required: mob.persistence_required,
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
            witch_using_item: mob.witch_drinking,
            pillager_charging: mob.crossbow.charging,
            effects: super::ctx::TargetEffects::from_map(&mob.effects),
            swimming: mob.swimming,
            in_water,
            skeleton_flag: match mob.mob_type {
                MobType::Skeleton => mob.freeze_conversion.converting,
                MobType::Bogged => mob.bogged_sheared,
                _ => false,
            },
            cube_size: mob.cube.size,
            equipment: mob
                .equipment
                .iter()
                .filter(|(_, stack)| !stack.is_empty())
                .map(|(slot, stack)| (slot, stack.clone()))
                .collect(),
            passengers: mob.passengers.clone(),
        }
    }

    pub fn dimensions(&self) -> (f32, f32) {
        if self.mob_type == MobType::Slime {
            let (width, height, _) = slime::dimensions(self.cube_size);
            return (width, height);
        }
        if self.mob_type == MobType::Phantom {
            let (width, height, _) = phantom::dimensions(self.phantom_size);
            return (width, height);
        }
        if self.mob_type == MobType::Goat {
            let (width, height, _) = goat::pose_dimensions(self.baby, self.goat.long_jumping);
            return (width, height);
        }
        if self.mob_type == MobType::Camel {
            let sitting = self.camel.pose == camel::Pose::Sitting;
            let (width, height, _) = camel::dimensions(self.baby, sitting);
            return (width, height);
        }
        if fish::is_fish(self.mob_type) {
            let (width, height, _) =
                fish::dimensions(self.mob_type, self.fish_variant, self.puff_state);
            return (width, height);
        }
        let (width, height, _) = self.mob_type.dimensions(self.baby);
        (width, height)
    }

    /// `Mob.isPersistenceRequired() || requiresCustomPersistence()`。
    ///
    /// `Mob.isPersistenceRequired() || requiresCustomPersistence()`.
    pub fn requires_custom_persistence(&self) -> bool {
        self.persistence_required
            || (fish::is_fish(self.mob_type) && self.fish_from_bucket)
            || self.enderman_carried.is_some()
    }

    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }
}

/// `makeBrain(Brain.Packed.EMPTY)`：跑大脑的种类建一个新大脑（传感器延迟还没取），
/// 用目标选择器的种类是 `None`。
///
/// `makeBrain(Brain.Packed.EMPTY)`: a fresh brain for brain-driven kinds (sensor delays
/// not drawn yet), `None` for goal-driven kinds.
pub(crate) fn new_brain(kind: MobType) -> Option<Box<brain::Brain>> {
    match kind {
        MobType::Armadillo => Some(Box::new(armadillo::brain())),
        MobType::Goat => Some(Box::new(goat::brain())),
        MobType::Frog => Some(Box::new(frog::brain())),
        MobType::Tadpole => Some(Box::new(tadpole::brain())),
        MobType::Camel => Some(Box::new(camel::brain())),
        MobType::Axolotl => Some(Box::new(axolotl::brain())),
        _ => None,
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
        assert!(MobPublic::of(&cow, false).baby);
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

    /// 每种 `MobType` 的注册表 ID 必须两两不同，且能从 `by_entity_type` 反查回自己。
    /// 新增生物时若漏进 `ALL` 或撞了已有 ID，官方刷怪表条目会被映射错（或漏映射），
    /// `natural-spawn` 关键路径随即分叉。
    ///
    /// Every `MobType` must have a distinct registry id that resolves back to
    /// itself via `by_entity_type`. When a kind is added, omitting it from `ALL`
    /// or reusing an existing id mismaps (or drops) a vanilla spawn-table entry
    /// and diverges the `natural-spawn` critical path.
    #[test]
    fn mob_type_registry_ids_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in MobType::ALL {
            let id = kind.entity_type();
            assert!(
                seen.insert(id),
                "duplicate entity_type id {id} for {kind:?}"
            );
            assert_eq!(
                MobType::by_entity_type(id),
                Some(kind),
                "by_entity_type({id}) does not resolve to {kind:?}"
            );
        }
        assert_eq!(seen.len(), MobType::ALL.len());
    }
}
