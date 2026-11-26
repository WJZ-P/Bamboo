//! 把实体介绍给客户端，以及移动之外的实体状态。

use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_nbt::Tag;
use uuid::Uuid;

use super::{HumanoidArm, write_lp_vec3};
use crate::{
    codec::{
        write_block_pos, write_bool, write_f32, write_f64, write_i32, write_string, write_uuid,
        write_varint, write_vec3,
    },
    item::ItemStack,
    packet::ClientboundPacket,
};

/// 实体元数据序列化器的 ID。顺序是 `EntityDataSerializers` 静态块里 `registerSerializer` 的调用顺序。
const SERIALIZER_BYTE: i32 = 0;
const SERIALIZER_INT: i32 = 1;
/// `EntityDataSerializers.LONG`：变长整数。
const SERIALIZER_LONG: i32 = 2;
const SERIALIZER_FLOAT: i32 = 3;
/// `EntityDataSerializers.ROTATIONS`，注册顺序里的第 10 个（ID 9）。
///
/// `EntityDataSerializers.ROTATIONS`, the tenth `registerSerializer` call (ID 9).
const SERIALIZER_ROTATIONS: i32 = 9;
/// `EntityDataSerializers.OPTIONAL_COMPONENT`，注册顺序里的第 7 个。
///
/// `EntityDataSerializers.OPTIONAL_COMPONENT`, the seventh `registerSerializer` call.
const SERIALIZER_OPTIONAL_COMPONENT: i32 = 6;
/// `EntityDataSerializers.ITEM_STACK`，注册顺序里的第 8 个。
///
/// `EntityDataSerializers.ITEM_STACK`, the eighth `registerSerializer` call.
const SERIALIZER_ITEM_STACK: i32 = 7;
const SERIALIZER_BOOLEAN: i32 = 8;
const SERIALIZER_BLOCK_POS: i32 = 10;
/// `EntityDataSerializers.OPTIONAL_BLOCK_POS`，注册顺序里的第 12 个（ID 11）。
///
/// `EntityDataSerializers.OPTIONAL_BLOCK_POS`, serializer id 11.
const SERIALIZER_OPTIONAL_BLOCK_POS: i32 = 11;
/// `EntityDataSerializers.DIRECTION`，注册顺序里的第 13 个（ID 12）。
///
/// `EntityDataSerializers.DIRECTION`, serializer id 12.
const SERIALIZER_DIRECTION: i32 = 12;
/// `EntityDataSerializers.OPTIONAL_LIVING_ENTITY_REFERENCE`，注册顺序第 13 个。
const SERIALIZER_OPTIONAL_LIVING_ENTITY_REFERENCE: i32 = 13;
const SERIALIZER_BLOCK_STATE: i32 = 14;
const SERIALIZER_OPTIONAL_BLOCK_STATE: i32 = 15;
/// `EntityDataSerializers.PARTICLES`：注册顺序里的第 18 个。
const SERIALIZER_PARTICLES: i32 = 17;
/// `EntityDataSerializers.OPTIONAL_UNSIGNED_INT`，注册顺序第 20 个（ID 19）。
const SERIALIZER_OPTIONAL_UNSIGNED_INT: i32 = 19;
const SERIALIZER_POSE: i32 = 20;
/// `EntityDataSerializers.COW_VARIANT`，注册顺序里的第 24 个。
///
/// `EntityDataSerializers.CAT_VARIANT` / `CAT_SOUND_VARIANT`（21、22 号注册）。
///
/// `EntityDataSerializers.CAT_VARIANT` and `CAT_SOUND_VARIANT` (serializer ids 21 and 22).
const SERIALIZER_CAT_VARIANT: i32 = 21;
const SERIALIZER_CAT_SOUND_VARIANT: i32 = 22;
/// `EntityDataSerializers.COW_VARIANT`, the 24th `registerSerializer` call.
const SERIALIZER_COW_VARIANT: i32 = 23;
const SERIALIZER_COW_SOUND_VARIANT: i32 = 24;
const SERIALIZER_WOLF_VARIANT: i32 = 25;
const SERIALIZER_WOLF_SOUND_VARIANT: i32 = 26;
/// `EntityDataSerializers.PAINTING_VARIANT`，注册顺序里的第 35 个（ID 34）。
///
/// `EntityDataSerializers.PAINTING_VARIANT`, serializer id 34.
const SERIALIZER_PAINTING_VARIANT: i32 = 34;
const SERIALIZER_HUMANOID_ARM: i32 = 42;

/// `HappyGhast` 在 `Animal` 之后定义的两项布尔元数据：
/// `Animal` 的 `DATA_BABY_ID`(16) / `AGE_LOCKED`(17) 之后依次是 18、19。
///
/// The two booleans `HappyGhast` defines after `Animal`: `DATA_BABY_ID`(16)
/// / `AGE_LOCKED`(17), then 18 and 19.
pub const HAPPY_GHAST_LEASH_HOLDER: u8 = 18;
pub const HAPPY_GHAST_STAYS_STILL: u8 = 19;

/// `set_entity_data` 的结束标记。
const ENTITY_DATA_END: u8 = 0xff;

/// `set_equipment` 里表示后面还有一项的位。
const EQUIPMENT_CONTINUE: u8 = 0x80;

/// 让客户端创建一个实体。
///
/// 玩家实体必须排在这名玩家的 `player_info_update`（带 `ADD_PLAYER`）之后，
/// 也不能发给玩家自己，否则客户端会把本地玩家删掉。
///
/// 位置和角度取上次发出去的状态，这样下一个位移才接得上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddEntity {
    pub entity_id: i32,
    pub uuid: Uuid,
    /// `minecraft:entity_type` 注册表 ID。
    pub entity_type: i32,
    pub position: Vec3,
    pub movement: Vec3,
    /// 这三个角度是 [`pack_degrees`](super::pack_degrees) 的结果。
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    pub data: i32,
}

impl ClientboundPacket for AddEntity {
    const ID: i32 = clientbound::ADD_ENTITY;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_uuid(out, self.uuid);
        write_varint(out, self.entity_type);
        write_vec3(out, self.position);
        write_lp_vec3(out, self.movement);
        out.extend_from_slice(&[self.pitch, self.yaw, self.head_yaw]);
        write_varint(out, self.data);
    }
}

/// 让客户端移除实体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveEntities<'a> {
    pub entity_ids: &'a [i32],
}

impl ClientboundPacket for RemoveEntities<'_> {
    const ID: i32 = clientbound::REMOVE_ENTITIES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_ids.len() as i32);
        for &id in self.entity_ids {
            write_varint(out, id);
        }
    }
}

/// 实体的姿势。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Standing = 0,
    FallFlying = 1,
    Sleeping = 2,
    Swimming = 3,
    SpinAttack = 4,
    Crouching = 5,
    /// 山羊长跳途中。
    ///
    /// A goat in mid long jump.
    LongJumping = 6,
    Dying = 7,
    /// 青蛙蹲叫。
    ///
    /// A croaking frog.
    Croaking = 8,
    /// 青蛙伸舌头。
    ///
    /// A frog using its tongue.
    UsingTongue = 9,
}

/// 玩家实体或物品实体的一项元数据。
///
/// 序列化器和 index 对不上时客户端会断线，所以每个 index 一个变体，不接受任意组合。
/// 物品堆不能 `Copy`，所以这个枚举也不再 `Copy`。
///
/// An item stack is not `Copy`, so neither is this enum.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityData {
    /// index 0，BYTE。`ENTITY_FLAG_*` 的组合。
    SharedFlags(u8),
    /// index 8，ITEM_STACK。地上的物品实体拿着的那一堆。
    ///
    /// index 8, ITEM_STACK. The stack a ground item entity is carrying.
    Item(ItemStack),
    /// index 9，OPTIONAL_UNSIGNED_INT。烟花火箭附着的目标实体 ID。
    ///
    /// index 9, OPTIONAL_UNSIGNED_INT. The firework rocket's attached entity id.
    FireworksAttached(Option<i32>),
    /// index 10，BOOLEAN。烟花火箭是不是斜射出去的（弩 / 发射器）。
    ///
    /// index 10, BOOLEAN. Whether the firework rocket was shot at an angle.
    FireworksShotAtAngle(bool),
    /// index 8，BLOCK_POS。下落的方块从哪一格掉下来。
    FallingBlockStart(BlockPos),
    /// index 8，INT。点燃的 TNT 还剩多少 tick 爆炸。
    TntFuse(i32),
    /// index 9，BLOCK_STATE。点燃的 TNT 渲染成什么方块。
    TntBlockState(i32),
    /// index 6，POSE。
    Pose(Pose),
    /// index 9，FLOAT。
    Health(f32),
    /// index 10，PARTICLES。`LivingEntity.DATA_EFFECT_PARTICLES`：
    /// 可见效果的颜色列表（`entity_effect` 粒子，VarInt 数量 + 每项 VarInt 粒子 ID + int ARGB）。
    ///
    /// index 10, PARTICLES: `DATA_EFFECT_PARTICLES`, the colours of visible effects.
    EffectParticles(Vec<i32>),
    /// index 11，BOOLEAN。`LivingEntity.DATA_EFFECT_AMBIENCE_ID`。
    EffectAmbience(bool),
    /// index 14，OPTIONAL_BLOCK_POS。`LivingEntity.SLEEPING_POS_ID`。
    ///
    /// index 14, OPTIONAL_BLOCK_POS. `LivingEntity.SLEEPING_POS_ID`.
    SleepingPos(Option<BlockPos>),
    /// index 15，HUMANOID_ARM。
    MainHand(HumanoidArm),
    /// index 16，BYTE。皮肤外层的掩码。
    ModelCustomisation(u8),
    /// index 1，INT。`DATA_AIR_SUPPLY_ID`。
    AirSupply(i32),
    /// index 2，OPTIONAL_COMPONENT。`DATA_CUSTOM_NAME`。
    ///
    /// index 2, OPTIONAL_COMPONENT. `DATA_CUSTOM_NAME`.
    CustomName(Tag),
    /// index 2，OPTIONAL_COMPONENT 为空：`CustomName` 被清掉时发这条。
    ///
    /// index 2, an empty OPTIONAL_COMPONENT: sent when `CustomName` is cleared.
    NoCustomName,
    /// index 3，BOOLEAN。`DATA_CUSTOM_NAME_VISIBLE`。
    CustomNameVisible(bool),
    /// index 4，BOOLEAN。`DATA_SILENT`。
    Silent(bool),
    /// index 5，BOOLEAN。`DATA_NO_GRAVITY`。
    NoGravity(bool),
    /// index 7，INT。`DATA_TICKS_FROZEN`。
    TicksFrozen(i32),
    /// index 8，BYTE。`LivingEntity.DATA_LIVING_ENTITY_FLAGS`。
    LivingFlags(u8),
    /// index 15，BYTE。`Mob.DATA_MOB_FLAGS_ID`：无 AI、左撇子、攻击性。
    ///
    /// index 15, BYTE. `Mob.DATA_MOB_FLAGS_ID`: no AI, left-handed, aggressive.
    MobFlags(u8),
    /// `Player.DATA_SHOULDER_PARROT_LEFT`（index 19）/ `RIGHT`（index 20），
    /// OPTIONAL_UNSIGNED_INT：肩膀上鹦鹉的品种 ID，空肩是 `None`。
    ///
    /// `Player.DATA_SHOULDER_PARROT_LEFT` / `RIGHT`: OPTIONAL_UNSIGNED_INT with
    /// the perched parrot's variant id, `None` for an empty shoulder.
    ShoulderParrotLeft(Option<i32>),
    ShoulderParrotRight(Option<i32>),
    /// index 16，BOOLEAN。`AgeableMob.DATA_BABY_ID`。
    Baby(bool),
    /// index 16，INT。`Warden.CLIENT_ANGER_LEVEL`（`getActiveAnger()`）。
    ///
    /// index 16, INT. `Warden.CLIENT_ANGER_LEVEL` (`getActiveAnger()`).
    ClientAngerLevel(i32),
    /// index 16，BOOLEAN。`Allay.DATA_DANCING`。
    AllayDancing(bool),
    /// index 17，BOOLEAN。`Allay.DATA_CAN_DUPLICATE`。
    AllayCanDuplicate(bool),
    /// index 17，BOOLEAN。`AgeableMob.AGE_LOCKED`。
    AgeLocked(bool),
    /// index 18，COW_VARIANT。`Cow.DATA_VARIANT_ID`，值是注册表 ID 的 VarInt。
    ///
    /// index 18, COW_VARIANT. `Cow.DATA_VARIANT_ID`, a VarInt registry id.
    CowVariant(i32),
    /// `Rabbit.DATA_TYPE_ID`：普通整数序列化器，值为旧版品种 ID。
    ///
    /// `Rabbit.DATA_TYPE_ID`: ordinary integer serializer, with a legacy variant ID.
    RabbitVariant(i32),
    /// index 19，COW_SOUND_VARIANT。`Cow.DATA_SOUND_VARIANT_ID`。
    CowSoundVariant(i32),
    PigBoostTime(i32),
    PigVariant(i32),
    PigSoundVariant(i32),
    /// `Sheep.DATA_WOOL_ID`（BYTE）：低 4 位颜色，第 5 位已剪毛。
    ///
    /// `Sheep.DATA_WOOL_ID` (BYTE): colour in the low four bits, bit 4 when sheared.
    SheepWool(u8),
    /// `Chicken.DATA_VARIANT_ID` / `DATA_SOUND_VARIANT_ID`，注册表 ID。
    ///
    /// `Chicken.DATA_VARIANT_ID` / `DATA_SOUND_VARIANT_ID` as registry IDs.
    ChickenVariant(i32),
    ChickenSoundVariant(i32),
    /// `Armadillo.ARMADILLO_STATE`：状态的网络 ID（`ARMADILLO_STATE` 序列化器，VarInt）。
    ///
    /// `Armadillo.ARMADILLO_STATE`: the state'"'"'s network id (its own serializer, a VarInt).
    ArmadilloState(i32),
    /// `CopperGolem.DATA_WEATHER_STATE`：`WeatherState` 的网络 ID（VarInt）。
    ///
    /// `CopperGolem.DATA_WEATHER_STATE`: the `WeatherState` network id (VarInt).
    CopperGolemWeatherState(i32),
    /// `CopperGolem.COPPER_GOLEM_STATE`：`CopperGolemState` 的网络 ID（VarInt）。
    ///
    /// `CopperGolem.COPPER_GOLEM_STATE`: the `CopperGolemState` network id (VarInt).
    CopperGolemState(i32),
    /// `Goat` 的三个 BOOLEAN：尖叫、左角、右角（缺省假、真、真）。
    ///
    /// The three `Goat` booleans: screaming, left horn and right horn (false, true and
    /// true by default).
    GoatScreaming(bool),
    GoatLeftHorn(bool),
    GoatRightHorn(bool),
    /// `Frog.DATA_VARIANT_ID`：`frog_variant` 注册表里的网络 ID。
    ///
    /// `Frog.DATA_VARIANT_ID`: the network id in the `frog_variant` registry.
    FrogVariant(i32),
    /// `Frog.DATA_TONGUE_TARGET_ID`（`OPTIONAL_UNSIGNED_INT`）：舌头目标的实体 ID。
    ///
    /// `Frog.DATA_TONGUE_TARGET_ID` (`OPTIONAL_UNSIGNED_INT`): the tongue target's
    /// entity id.
    FrogTongueTarget(Option<i32>),
    /// `Sniffer.DATA_STATE`：`Sniffer.State` 的网络 ID（自己的 `SNIFFER_STATE` 序列化器，VarInt）。
    ///
    /// `Sniffer.DATA_STATE`: the `Sniffer.State` network id (its own `SNIFFER_STATE`
    /// serializer, a VarInt).
    SnifferState(i32),
    /// `Sniffer.DATA_DROP_SEED_AT_TICK`，INT。
    ///
    /// `Sniffer.DATA_DROP_SEED_AT_TICK`, an INT.
    SnifferDropSeedAtTick(i32),
    /// `MushroomCow.DATA_TYPE`（INT）：0 红色，1 棕色。
    ///
    /// `MushroomCow.DATA_TYPE` (INT): 0 red, 1 brown.
    MooshroomType(i32),
    /// `PolarBear.DATA_STANDING_ID`（BOOLEAN）：站起来警告。
    ///
    /// `PolarBear.DATA_STANDING_ID` (BOOLEAN): standing up to warn.
    PolarBearStanding(bool),
    /// 熊猫的三个计数器（INT）。
    ///
    /// The three panda counters (INT).
    PandaUnhappy(i32),
    PandaSneeze(i32),
    PandaEat(i32),
    /// 熊猫的显性 / 隐性基因（BYTE，`Panda.Gene` 的 id）。
    ///
    /// The panda main and hidden genes (BYTE, a `Panda.Gene` id).
    PandaMainGene(u8),
    PandaHiddenGene(u8),
    /// `Panda.DATA_ID_FLAGS`（BYTE）：2 打喷嚏、4 打滚、8 坐下、16 仰躺。
    ///
    /// `Panda.DATA_ID_FLAGS` (BYTE): sneezing, rolling, sitting, on its back.
    PandaFlags(u8),
    /// `GlowSquid.DATA_DARK_TICKS_REMAINING`，INT。
    ///
    /// `GlowSquid.DATA_DARK_TICKS_REMAINING`, an INT.
    GlowSquidDarkTicks(i32),
    /// `Bee.DATA_FLAGS_ID`，BYTE：2 翻滚、4 已蜇、8 有花蜜。
    ///
    /// `Bee.DATA_FLAGS_ID`, a BYTE: rolling, stung and nectar bits.
    BeeFlags(u8),
    /// `Bee.DATA_ANGER_END_TIME`，LONG（变长）。
    BeeAngerEndTime(i64),
    /// `IronGolem.DATA_FLAGS_ID` / `SnowGolem.DATA_PUMPKIN_ID`，都是 BYTE。
    ///
    /// Both golem flag bytes share the same index and serializer.
    GolemFlags(u8),
    /// `Bat.DATA_ID_FLAGS`，BYTE：第 0 位是倒挂。
    ///
    /// `Bat.DATA_ID_FLAGS`, a BYTE; bit 0 is resting.
    BatFlags(u8),
    /// `Phantom.ID_SIZE`，INT。
    ///
    /// `Phantom.ID_SIZE`, an INT.
    PhantomSize(i32),
    /// `AbstractFish.FROM_BUCKET`，BOOLEAN，index 16。
    ///
    /// `AbstractFish.FROM_BUCKET`, a BOOLEAN at index 16.
    FishFromBucket(bool),
    /// `Salmon.DATA_TYPE` / `TropicalFish.DATA_ID_TYPE_VARIANT` / `Pufferfish.PUFF_STATE`，
    /// 都在 index 17，INT。
    ///
    /// `Salmon.DATA_TYPE` / `TropicalFish.DATA_ID_TYPE_VARIANT` / `Pufferfish.PUFF_STATE`,
    /// all at index 17 as an INT.
    SalmonVariant(i32),
    TropicalFishVariant(i32),
    PufferfishPuffState(i32),
    /// `Turtle.HAS_EGG` / `Turtle.LAYING_EGG`，BOOLEAN。
    ///
    /// `Turtle.HAS_EGG` / `Turtle.LAYING_EGG`, BOOLEANs.
    TurtleHasEgg(bool),
    TurtleLayingEgg(bool),
    /// `Dolphin.GOT_FISH`，BOOLEAN，index 18。
    ///
    /// `Dolphin.GOT_FISH`, a BOOLEAN at index 18.
    DolphinGotFish(bool),
    /// `Dolphin.MOISTNESS_LEVEL`，INT，index 19。
    ///
    /// `Dolphin.MOISTNESS_LEVEL`, an INT at index 19.
    DolphinMoistness(i32),
    /// `Axolotl.DATA_VARIANT`，INT，index 18。
    ///
    /// `Axolotl.DATA_VARIANT`, an INT at index 18.
    AxolotlVariant(i32),
    /// `Axolotl.DATA_PLAYING_DEAD`，BOOLEAN，index 19。
    ///
    /// `Axolotl.DATA_PLAYING_DEAD`, a BOOLEAN at index 19.
    AxolotlPlayingDead(bool),
    /// `Axolotl.FROM_BUCKET`，BOOLEAN，index 20。
    ///
    /// `Axolotl.FROM_BUCKET`, a BOOLEAN at index 20.
    AxolotlFromBucket(bool),
    /// `Villager.DATA_VILLAGER_DATA`（index 19，`VILLAGER_DATA` 序列化器）：
    /// 品种、职业、等级三个 VarInt。序列化器的 ID 从官方字节码导出。
    ///
    /// `Villager.DATA_VILLAGER_DATA` (index 19, the `VILLAGER_DATA` serializer):
    /// the kind, profession and level as three VarInts.
    VillagerData {
        kind: i32,
        profession: i32,
        level: i32,
    },
    /// `Villager.DATA_VILLAGER_DATA_FINALIZED`（index 20，BOOLEAN）。
    ///
    /// `Villager.DATA_VILLAGER_DATA_FINALIZED` (index 20, a BOOLEAN).
    VillagerDataFinalized(bool),
    /// `AbstractVillager.DATA_UNHAPPY_COUNTER`（index 18，INT）。
    ///
    /// `AbstractVillager.DATA_UNHAPPY_COUNTER` (index 18, an INT).
    VillagerUnhappy(i32),
    /// `ZombieVillager.DATA_CONVERTING_ID`（index 19，BOOLEAN）。
    ZombieVillagerConverting(bool),
    /// `ZombieVillager.DATA_VILLAGER_DATA`（index 20）。
    ZombieVillagerData {
        kind: i32,
        profession: i32,
        level: i32,
    },
    /// `ZombieVillager.DATA_VILLAGER_DATA_FINALIZED`（index 21，BOOLEAN）。
    ZombieVillagerDataFinalized(bool),
    /// index 8，INT。`ExperienceOrb.DATA_VALUE`：这一球值多少经验。
    ///
    /// index 8, INT. `ExperienceOrb.DATA_VALUE`, this orb's experience value.
    ExperienceValue(i32),
    /// index 17，INT。`Zombie.DATA_SPECIAL_TYPE_ID`。
    SpecialType(i32),
    /// `AbstractCubeMob.ID_SIZE`（INT）：史莱姆 / 硫磺方块怪的大小。
    ///
    /// `AbstractCubeMob.ID_SIZE` (INT): the slime / sulfur cube size.
    CubeMobSize(i32),
    /// `SulfurCube.MAX_FUSE`（index 19，INT，缺省 -1）。
    SulfurCubeMaxFuse(i32),
    /// `SulfurCube.FROM_BUCKET`（index 20，BOOLEAN）。
    SulfurCubeFromBucket(bool),
    /// index 18，BOOLEAN。`Zombie.DATA_DROWNED_CONVERSION_ID`。
    DrownedConverting(bool),
    /// index 17，BOOLEAN。`Witch.DATA_USING_ITEM`（`Entity` 0–7、`LivingEntity` 8–14、
    /// `Mob` 15、`Raider.IS_CELEBRATING` 16、`Witch.DATA_USING_ITEM` 17）。
    ///
    /// index 17, BOOLEAN. `Witch.DATA_USING_ITEM` (Entity 0–7, LivingEntity 8–14, Mob 15,
    /// Raider.IS_CELEBRATING 16, Witch.DATA_USING_ITEM 17).
    WitchUsingItem(bool),
    /// `Creeper.DATA_SWELL_DIR`（INT，缺省 -1）、`DATA_IS_POWERED`、`DATA_IS_IGNITED`（BOOLEAN）。
    ///
    /// `Creeper.DATA_SWELL_DIR` (INT, default -1), `DATA_IS_POWERED` and `DATA_IS_IGNITED`.
    CreeperSwellDir(i32),
    CreeperPowered(bool),
    CreeperIgnited(bool),
    /// `Spider.DATA_FLAGS_ID`（BYTE，缺省 0，第 0 位是攀爬）。
    ///
    /// `Spider.DATA_FLAGS_ID` (BYTE, default 0, bit 0 is climbing).
    SpiderFlags(u8),
    EndermanCarried(Option<i32>),
    EndermanCreepy(bool),
    EndermanStaredAt(bool),
    /// `Ocelot.DATA_TRUSTING`：BOOLEAN。
    ///
    /// `Ocelot.DATA_TRUSTING`: BOOLEAN.
    OcelotTrusting(bool),
    /// `Cat.DATA_VARIANT_ID`：`cat_variant` 注册表 ID。
    ///
    /// `Cat.DATA_VARIANT_ID`: `cat_variant` registry id.
    CatVariant(i32),
    /// `Cat.IS_LYING`：BOOLEAN。
    ///
    /// `Cat.IS_LYING`: BOOLEAN.
    CatIsLying(bool),
    /// `Cat.RELAX_STATE_ONE`：BOOLEAN。
    ///
    /// `Cat.RELAX_STATE_ONE`: BOOLEAN.
    CatRelaxStateOne(bool),
    /// `Cat.DATA_COLLAR_COLOR`：INT（`DyeColor` ID）。
    ///
    /// `Cat.DATA_COLLAR_COLOR`: INT (`DyeColor` id).
    CatCollar(i32),
    /// `Cat.DATA_SOUND_VARIANT_ID`：`cat_sound_variant` 注册表 ID。
    ///
    /// `Cat.DATA_SOUND_VARIANT_ID`: `cat_sound_variant` registry id.
    CatSoundVariant(i32),
    /// `Parrot.DATA_VARIANT_ID`：INT，0..=4。
    ///
    /// `Parrot.DATA_VARIANT_ID`: INT, 0..=4.
    ParrotVariant(i32),
    ArrowFlags(u8),
    ArrowPierce(i8),
    ArrowInGround(bool),
    /// `Fox.DATA_TYPE_ID`：INT，红 0 雪 1。
    ///
    /// `Fox.DATA_TYPE_ID`: INT, red 0 and snow 1.
    FoxType(i32),
    /// `Fox.DATA_FLAGS_ID`：BYTE。
    FoxFlags(u8),
    /// `Fox.DATA_TRUSTED_ID_0` / `_1`：OPTIONAL_LIVING_ENTITY_REFERENCE（可空 UUID）。
    ///
    /// `Fox.DATA_TRUSTED_ID_0` / `_1`: an optional UUID reference.
    FoxTrusted(u8, Option<[u8; 16]>),
    /// BYTE。`AbstractHorse.DATA_ID_FLAGS`：2 驯服、8 繁殖过、16 吃草、32 扬蹄、64 张嘴。
    ///
    /// BYTE. `AbstractHorse.DATA_ID_FLAGS`: tame, bred, eating, standing, open mouth.
    HorseFlags(u8),
    /// INT。`Horse.DATA_ID_TYPE_VARIANT`：低字节毛色，第二个字节斑纹。
    ///
    /// INT. `Horse.DATA_ID_TYPE_VARIANT`: coat in the low byte, markings in the next.
    HorseVariant(i32),
    /// BOOLEAN。`AbstractChestedHorse.DATA_ID_CHEST`。
    HorseChest(bool),
    /// INT。`Llama.DATA_STRENGTH_ID`。
    LlamaStrength(i32),
    /// INT。`Llama.DATA_VARIANT_ID`。
    LlamaVariant(i32),
    ArrowColor(i32),
    /// `ThrownTrident.DATA_ID_LOYALTY`：BYTE，忠诚等级（索引 11，只在三叉戟上用）。
    ///
    /// `ThrownTrident.DATA_ID_LOYALTY`: BYTE, the loyalty level (index 11, tridents only).
    TridentLoyalty(u8),
    /// `ThrownTrident.DATA_ID_FOIL`：BOOLEAN，附魔光效（索引 12）。
    ///
    /// `ThrownTrident.DATA_ID_FOIL`: BOOLEAN, the enchantment glint (index 12).
    TridentFoil(bool),
    /// `TamableAnimal.DATA_FLAGS_ID`：BYTE，坐姿 1、驯服 4。
    ///
    /// `TamableAnimal.DATA_FLAGS_ID`: BYTE, sitting 1 and tame 4.
    TameFlags(u8),
    /// `TamableAnimal.DATA_OWNERUUID_ID`：可空的 UUID 引用。
    ///
    /// `TamableAnimal.DATA_OWNERUUID_ID`: an optional UUID reference.
    TameOwner(Option<[u8; 16]>),
    /// `Wolf.DATA_INTERESTED_ID`：BOOLEAN。
    WolfInterested(bool),
    /// `Wolf.DATA_COLLAR_COLOR`：INT，`DyeColor` 的 ID。
    WolfCollar(i32),
    /// `Wolf.DATA_ANGER_END_TIME`：LONG（变长）。
    WolfAngerEndTime(i64),
    /// `Wolf.DATA_VARIANT_ID`：`wolf_variant` 注册表 ID。
    WolfVariant(i32),
    /// `Wolf.DATA_SOUND_VARIANT_ID`：`wolf_sound_variant` 注册表 ID。
    WolfSoundVariant(i32),
    /// `AbstractNautilus.DASH`（index 20，BOOLEAN）。
    ///
    /// `AbstractNautilus.DASH` (index 20, BOOLEAN).
    NautilusDash(bool),
    /// `ZombieNautilus.DATA_VARIANT_ID`（index 21，注册表 ID）。
    ///
    /// `ZombieNautilus.DATA_VARIANT_ID` (index 21, a registry id).
    ZombieNautilusVariant(i32),
    /// `HappyGhast.IS_LEASH_HOLDER`（BOOLEAN，index 18，缺省假）。
    ///
    /// `HappyGhast.IS_LEASH_HOLDER` (BOOLEAN, index 18, false by default).
    HappyGhastLeashHolder(bool),
    /// `HappyGhast.STAYS_STILL`（BOOLEAN，index 19，缺省假）。
    ///
    /// `HappyGhast.STAYS_STILL` (BOOLEAN, index 19, false by default).
    HappyGhastStaysStill(bool),
    /// `Creaking.CAN_MOVE`（BOOLEAN，索引 16，缺省 true）。
    CreakingCanMove(bool),
    /// `Creaking.IS_ACTIVE`（BOOLEAN，索引 17，缺省 false）。
    CreakingActive(bool),
    /// `Creaking.IS_TEARING_DOWN`（BOOLEAN，索引 18，缺省 false）。
    CreakingTearingDown(bool),
    /// `Creaking.HOME_POS`（OPTIONAL_BLOCK_POS，索引 19，缺省 None）。
    CreakingHomePos(Option<BlockPos>),
    /// `Guardian.DATA_ID_MOVING`（BOOLEAN，索引 16）。
    ///
    /// `Guardian.DATA_ID_MOVING` (BOOLEAN, index 16).
    GuardianMoving(bool),
    /// `Guardian.DATA_ID_ATTACK_TARGET`（INT，索引 17）。
    ///
    /// `Guardian.DATA_ID_ATTACK_TARGET` (INT, index 17).
    GuardianAttackTarget(i32),
    /// `ArmorStand.DATA_CLIENT_FLAGS`（BYTE，索引 15）：小型、手臂、无底座、标记。
    ///
    /// `ArmorStand.DATA_CLIENT_FLAGS` (BYTE, index 15): small, arms, no baseplate, marker.
    ArmorStandFlags(u8),
    /// 六个 `ArmorStand` 姿势（ROTATIONS，索引 16..21）：`[x, y, z]` 三轴旋转（度）。
    ///
    /// The six `ArmorStand` poses (ROTATIONS, index 16..21): `[x, y, z]` degrees.
    ArmorStandHeadPose([f32; 3]),
    ArmorStandBodyPose([f32; 3]),
    ArmorStandLeftArmPose([f32; 3]),
    ArmorStandRightArmPose([f32; 3]),
    ArmorStandLeftLegPose([f32; 3]),
    ArmorStandRightLegPose([f32; 3]),
    /// `HangingEntity.DATA_DIRECTION`（DIRECTION，索引 8，缺省 `Direction::South`）。
    ///
    /// `HangingEntity.DATA_DIRECTION` (DIRECTION, index 8, default `Direction::South`).
    HangingDirection(Direction),
    /// `ItemFrame.DATA_ITEM`（ITEM_STACK，索引 9，缺省空堆）。
    ///
    /// `ItemFrame.DATA_ITEM` (ITEM_STACK, index 9, default empty).
    ItemFrameItem(ItemStack),
    /// `ItemFrame.DATA_ROTATION`（INT，索引 10，缺省 0）。
    ///
    /// `ItemFrame.DATA_ROTATION` (INT, index 10, default 0).
    ItemFrameRotation(i32),
    /// `Painting.DATA_PAINTING_VARIANT_ID`（PAINTING_VARIANT，索引 9，注册表 ID，网络写 `id + 1`）。
    ///
    /// `Painting.DATA_PAINTING_VARIANT_ID` (PAINTING_VARIANT, index 9, registry id written as `id + 1`).
    PaintingVariant(i32),
}

/// [`EntityData::SharedFlags`] 里表示按住潜行键的位。
pub const ENTITY_FLAG_SHIFT_KEY_DOWN: u8 = 0x02;
/// [`EntityData::SharedFlags`] 里表示正在冲刺的位。
pub const ENTITY_FLAG_SPRINTING: u8 = 0x08;
/// [`EntityData::SharedFlags`] 里表示隐身的位（`Entity.FLAG_INVISIBLE = 5`）。
///
/// The invisibility bit in [`EntityData::SharedFlags`] (`Entity.FLAG_INVISIBLE = 5`).
pub const ENTITY_FLAG_INVISIBLE_BIT: u8 = 0x20;
/// [`EntityData::SharedFlags`] 里表示正在滑翔的位。
pub const ENTITY_FLAG_FALL_FLYING: u8 = 0x80;
/// `Entity.FLAG_INVISIBLE`：`DATA_SHARED_FLAGS_ID` 的 bit 5。
pub const ENTITY_FLAG_INVISIBLE: u8 = 0x20;

impl EntityData {
    /// 元数据的 index；比较是不是同一项时用它，配对时按它升序。
    ///
    /// The metadata index; used to compare entries and to keep pairing order.
    pub const fn index(&self) -> u8 {
        match self {
            Self::SharedFlags(_) => 0,
            Self::AirSupply(_) => 1,
            Self::CustomName(_) | Self::NoCustomName => 2,
            Self::CustomNameVisible(_) => 3,
            Self::Silent(_) => 4,
            Self::NoGravity(_) => 5,
            Self::Pose(_) => 6,
            Self::TicksFrozen(_) => 7,
            Self::Item(_)
            | Self::FallingBlockStart(_)
            | Self::TntFuse(_)
            | Self::LivingFlags(_)
            | Self::ExperienceValue(_)
            | Self::HangingDirection(_) => 8,
            Self::ArrowFlags(_) => mc_bamboo_data::entity_types::ARROW_FLAGS,
            Self::ArrowPierce(_) => mc_bamboo_data::entity_types::ARROW_PIERCE,
            Self::ArrowInGround(_) => mc_bamboo_data::entity_types::ARROW_IN_GROUND,
            Self::HorseFlags(_) => mc_bamboo_data::entity_types::HORSE_FLAGS,
            Self::HorseVariant(_) => mc_bamboo_data::entity_types::HORSE_VARIANT,
            Self::HorseChest(_) => mc_bamboo_data::entity_types::HORSE_CHEST,
            Self::LlamaStrength(_) => mc_bamboo_data::entity_types::LLAMA_STRENGTH,
            Self::LlamaVariant(_) => mc_bamboo_data::entity_types::LLAMA_VARIANT,
            Self::ArrowColor(_) => mc_bamboo_data::entity_types::ARROW_COLOR,
            Self::TridentLoyalty(_) => mc_bamboo_data::entity_types::TRIDENT_LOYALTY,
            Self::TridentFoil(_) => mc_bamboo_data::entity_types::TRIDENT_FOIL,
            Self::TntBlockState(_)
            | Self::Health(_)
            | Self::FireworksAttached(_)
            | Self::ItemFrameItem(_)
            | Self::PaintingVariant(_) => 9,
            Self::EffectParticles(_)
            | Self::FireworksShotAtAngle(_)
            | Self::ItemFrameRotation(_) => 10,
            Self::EffectAmbience(_) => 11,
            Self::SleepingPos(_) => 14,
            Self::MobFlags(_) => 15,
            Self::MainHand(_)
            | Self::Baby(_)
            | Self::ClientAngerLevel(_)
            | Self::AllayDancing(_) => 16,
            Self::ModelCustomisation(_)
            | Self::AgeLocked(_)
            | Self::AllayCanDuplicate(_)
            | Self::SpecialType(_)
            | Self::WitchUsingItem(_) => 17,
            Self::CowVariant(_) | Self::DrownedConverting(_) => 18,
            // 村民的 18 是 `DATA_UNHAPPY_COUNTER`（INT），不是牛的花色。
            //
            // A villager's 18 is `DATA_UNHAPPY_COUNTER` (an INT), not the cow variant.
            Self::VillagerUnhappy(_) => mc_bamboo_data::villager::UNHAPPY_COUNTER_INDEX,
            Self::VillagerData { .. } => mc_bamboo_data::villager::VILLAGER_DATA_INDEX,
            Self::VillagerDataFinalized(_) => {
                mc_bamboo_data::villager::VILLAGER_DATA_FINALIZED_INDEX
            }
            Self::ZombieVillagerConverting(_) => {
                mc_bamboo_data::villager::ZOMBIE_VILLAGER_CONVERTING_INDEX
            }
            Self::ZombieVillagerData { .. } => mc_bamboo_data::villager::ZOMBIE_VILLAGER_DATA_INDEX,
            Self::ZombieVillagerDataFinalized(_) => {
                mc_bamboo_data::villager::ZOMBIE_VILLAGER_DATA_FINALIZED_INDEX
            }
            Self::RabbitVariant(_) => mc_bamboo_data::entity_types::RABBIT_VARIANT,
            Self::CowSoundVariant(_) => 19,
            Self::ShoulderParrotLeft(_) => 19,
            Self::ShoulderParrotRight(_) => 20,
            Self::CreeperSwellDir(_) => mc_bamboo_data::entity_types::CREEPER_SWELL_DIR,
            Self::CreeperPowered(_) => mc_bamboo_data::entity_types::CREEPER_POWERED,
            Self::CreeperIgnited(_) => mc_bamboo_data::entity_types::CREEPER_IGNITED,
            Self::FoxType(_) => mc_bamboo_data::entity_types::FOX_TYPE,
            Self::FoxFlags(_) => mc_bamboo_data::entity_types::FOX_FLAGS,
            Self::FoxTrusted(index, _) => *index,
            Self::TameFlags(_) => mc_bamboo_data::entity_types::TAME_FLAGS,
            Self::TameOwner(_) => mc_bamboo_data::entity_types::TAME_OWNER,
            Self::WolfInterested(_) => mc_bamboo_data::entity_types::WOLF_INTERESTED,
            Self::WolfCollar(_) => mc_bamboo_data::entity_types::WOLF_COLLAR,
            Self::WolfAngerEndTime(_) => mc_bamboo_data::entity_types::WOLF_ANGER_END_TIME,
            Self::WolfVariant(_) => mc_bamboo_data::entity_types::WOLF_VARIANT,
            Self::WolfSoundVariant(_) => mc_bamboo_data::entity_types::WOLF_SOUND_VARIANT,
            Self::PigBoostTime(_) => mc_bamboo_data::entity_types::PIG_BOOST_TIME,
            Self::PigVariant(_) => mc_bamboo_data::entity_types::PIG_VARIANT,
            Self::PigSoundVariant(_) => mc_bamboo_data::entity_types::PIG_SOUND_VARIANT,
            Self::SheepWool(_) => mc_bamboo_data::entity_types::SHEEP_WOOL,
            Self::CubeMobSize(_) => mc_bamboo_data::entity_types::CUBE_MOB_SIZE,
            Self::SulfurCubeMaxFuse(_) => mc_bamboo_data::entity_types::SULFUR_CUBE_MAX_FUSE,
            Self::SulfurCubeFromBucket(_) => mc_bamboo_data::entity_types::SULFUR_CUBE_FROM_BUCKET,
            Self::ChickenVariant(_) => mc_bamboo_data::entity_types::CHICKEN_VARIANT,
            Self::ChickenSoundVariant(_) => mc_bamboo_data::entity_types::CHICKEN_SOUND_VARIANT,
            Self::ArmadilloState(_) => mc_bamboo_data::entity_types::ARMADILLO_STATE,
            Self::CopperGolemWeatherState(_) => {
                mc_bamboo_data::entity_types::COPPER_GOLEM_WEATHER_STATE
            }
            Self::CopperGolemState(_) => mc_bamboo_data::entity_types::COPPER_GOLEM_STATE,
            Self::GoatScreaming(_) => mc_bamboo_data::entity_types::GOAT_SCREAMING,
            Self::GoatLeftHorn(_) => mc_bamboo_data::entity_types::GOAT_LEFT_HORN,
            Self::GoatRightHorn(_) => mc_bamboo_data::entity_types::GOAT_RIGHT_HORN,
            Self::FrogVariant(_) => mc_bamboo_data::entity_types::FROG_VARIANT,
            Self::FrogTongueTarget(_) => mc_bamboo_data::entity_types::FROG_TONGUE_TARGET,
            Self::SnifferState(_) => mc_bamboo_data::entity_types::SNIFFER_STATE,
            Self::SnifferDropSeedAtTick(_) => {
                mc_bamboo_data::entity_types::SNIFFER_DROP_SEED_AT_TICK
            }
            Self::MooshroomType(_) => mc_bamboo_data::entity_types::MOOSHROOM_TYPE,
            Self::PolarBearStanding(_) => mc_bamboo_data::entity_types::POLAR_BEAR_STANDING,
            Self::PandaUnhappy(_) => mc_bamboo_data::entity_types::PANDA_UNHAPPY,
            Self::PandaSneeze(_) => mc_bamboo_data::entity_types::PANDA_SNEEZE,
            Self::PandaEat(_) => mc_bamboo_data::entity_types::PANDA_EAT,
            Self::PandaMainGene(_) => mc_bamboo_data::entity_types::PANDA_MAIN_GENE,
            Self::PandaHiddenGene(_) => mc_bamboo_data::entity_types::PANDA_HIDDEN_GENE,
            Self::PandaFlags(_) => mc_bamboo_data::entity_types::PANDA_FLAGS,
            Self::OcelotTrusting(_) => mc_bamboo_data::entity_types::OCELOT_TRUSTING,
            Self::CatVariant(_) => mc_bamboo_data::entity_types::CAT_VARIANT,
            Self::CatIsLying(_) => mc_bamboo_data::entity_types::CAT_IS_LYING,
            Self::CatRelaxStateOne(_) => mc_bamboo_data::entity_types::CAT_RELAX_STATE_ONE,
            Self::CatCollar(_) => mc_bamboo_data::entity_types::CAT_COLLAR,
            Self::CatSoundVariant(_) => mc_bamboo_data::entity_types::CAT_SOUND_VARIANT,
            Self::ParrotVariant(_) => mc_bamboo_data::entity_types::PARROT_VARIANT,
            Self::EndermanCarried(_) => mc_bamboo_data::entity_types::ENDERMAN_CARRY_STATE,
            Self::EndermanCreepy(_) => mc_bamboo_data::entity_types::ENDERMAN_CREEPY,
            Self::EndermanStaredAt(_) => mc_bamboo_data::entity_types::ENDERMAN_STARED_AT,
            Self::SpiderFlags(_) => mc_bamboo_data::entity_types::SPIDER_FLAGS,
            Self::GlowSquidDarkTicks(_) => mc_bamboo_data::entity_types::GLOW_SQUID_DARK_TICKS,
            Self::BeeFlags(_) => mc_bamboo_data::entity_types::BEE_FLAGS,
            Self::BeeAngerEndTime(_) => mc_bamboo_data::entity_types::BEE_ANGER_END_TIME,
            Self::GolemFlags(_) => mc_bamboo_data::entity_types::IRON_GOLEM_FLAGS,
            Self::BatFlags(_) => mc_bamboo_data::entity_types::BAT_FLAGS,
            Self::PhantomSize(_) => mc_bamboo_data::entity_types::PHANTOM_SIZE,
            Self::FishFromBucket(_) => mc_bamboo_data::entity_types::FISH_FROM_BUCKET,
            Self::SalmonVariant(_) => mc_bamboo_data::entity_types::SALMON_VARIANT,
            Self::TropicalFishVariant(_) => mc_bamboo_data::entity_types::TROPICAL_FISH_VARIANT,
            Self::PufferfishPuffState(_) => mc_bamboo_data::entity_types::PUFFERFISH_PUFF_STATE,
            Self::TurtleHasEgg(_) => mc_bamboo_data::entity_types::TURTLE_HAS_EGG,
            Self::TurtleLayingEgg(_) => mc_bamboo_data::entity_types::TURTLE_LAYING_EGG,
            Self::DolphinGotFish(_) => mc_bamboo_data::entity_types::DOLPHIN_GOT_FISH,
            Self::DolphinMoistness(_) => mc_bamboo_data::entity_types::DOLPHIN_MOISTNESS,
            Self::AxolotlVariant(_) => mc_bamboo_data::entity_types::AXOLOTL_VARIANT,
            Self::AxolotlPlayingDead(_) => mc_bamboo_data::entity_types::AXOLOTL_PLAYING_DEAD,
            Self::AxolotlFromBucket(_) => mc_bamboo_data::entity_types::AXOLOTL_FROM_BUCKET,
            Self::NautilusDash(_) => mc_bamboo_data::entity_types::NAUTILUS_DASH,
            Self::ZombieNautilusVariant(_) => mc_bamboo_data::entity_types::ZOMBIE_NAUTILUS_VARIANT,
            Self::GuardianMoving(_) => mc_bamboo_data::entity_types::GUARDIAN_MOVING,
            Self::GuardianAttackTarget(_) => mc_bamboo_data::entity_types::GUARDIAN_ATTACK_TARGET,
            Self::HappyGhastLeashHolder(_) => HAPPY_GHAST_LEASH_HOLDER,
            Self::HappyGhastStaysStill(_) => HAPPY_GHAST_STAYS_STILL,
            Self::CreakingCanMove(_) => mc_bamboo_data::entity_types::CREAKING_CAN_MOVE,
            Self::CreakingActive(_) => mc_bamboo_data::entity_types::CREAKING_IS_ACTIVE,
            Self::CreakingTearingDown(_) => mc_bamboo_data::entity_types::CREAKING_IS_TEARING_DOWN,
            Self::CreakingHomePos(_) => mc_bamboo_data::entity_types::CREAKING_HOME_POS,
            Self::ArmorStandFlags(_) => 15,
            Self::ArmorStandHeadPose(_) => 16,
            Self::ArmorStandBodyPose(_) => 17,
            Self::ArmorStandLeftArmPose(_) => 18,
            Self::ArmorStandRightArmPose(_) => 19,
            Self::ArmorStandLeftLegPose(_) => 20,
            Self::ArmorStandRightLegPose(_) => 21,
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::ShoulderParrotLeft(variant)
            | Self::ShoulderParrotRight(variant)
            | Self::FireworksAttached(variant) => {
                write_data_header(out, self.index(), SERIALIZER_OPTIONAL_UNSIGNED_INT);
                // 官方 OptionalInt：空写 0，有值写品种 ID + 1。
                write_varint(out, variant.map_or(0, |value| value + 1));
            }
            Self::TameFlags(value) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*value);
            }
            Self::TameOwner(uuid) => {
                write_data_header(
                    out,
                    self.index(),
                    SERIALIZER_OPTIONAL_LIVING_ENTITY_REFERENCE,
                );
                out.push(u8::from(uuid.is_some()));
                if let Some(uuid) = uuid {
                    out.extend_from_slice(uuid);
                }
            }
            Self::WolfInterested(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::WolfCollar(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::WolfAngerEndTime(value) | Self::BeeAngerEndTime(value) => {
                write_data_header(out, self.index(), SERIALIZER_LONG);
                crate::codec::write_varlong(out, *value);
            }
            Self::WolfVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_WOLF_VARIANT);
                write_varint(out, *value);
            }
            Self::WolfSoundVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_WOLF_SOUND_VARIANT);
                write_varint(out, *value);
            }
            Self::FoxType(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::FoxFlags(value) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*value);
            }
            Self::FoxTrusted(_, uuid) => {
                write_data_header(
                    out,
                    self.index(),
                    SERIALIZER_OPTIONAL_LIVING_ENTITY_REFERENCE,
                );
                out.push(u8::from(uuid.is_some()));
                if let Some(uuid) = uuid {
                    out.extend_from_slice(uuid);
                }
            }
            Self::ArrowFlags(value) | Self::TridentLoyalty(value) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*value);
            }
            Self::ArrowPierce(value) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*value as u8);
            }
            Self::ArrowColor(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::AllayDancing(value) | Self::AllayCanDuplicate(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                out.push(u8::from(*value));
            }
            Self::ArrowInGround(value)
            | Self::TridentFoil(value)
            | Self::HorseChest(value)
            | Self::GoatScreaming(value)
            | Self::GoatLeftHorn(value)
            | Self::GoatRightHorn(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                out.push(u8::from(*value));
            }
            Self::HorseFlags(flags) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::HorseVariant(value) | Self::LlamaStrength(value) | Self::LlamaVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::Item(stack) => {
                write_data_header(out, 8, SERIALIZER_ITEM_STACK);
                // 组件按官方原样发给所有观察者。
                // Components are forwarded to every viewer exactly as vanilla does.
                stack.write(out);
            }
            Self::SharedFlags(flags) => {
                write_data_header(out, 0, SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::FallingBlockStart(pos) => {
                write_data_header(out, 8, SERIALIZER_BLOCK_POS);
                write_block_pos(out, *pos);
            }
            Self::TntFuse(fuse) => {
                write_data_header(out, 8, SERIALIZER_INT);
                write_varint(out, *fuse);
            }
            Self::ExperienceValue(value) => {
                write_data_header(out, 8, SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::TntBlockState(state) => {
                write_data_header(out, 9, SERIALIZER_BLOCK_STATE);
                write_varint(out, *state);
            }
            Self::Pose(pose) => {
                write_data_header(out, 6, SERIALIZER_POSE);
                write_varint(out, *pose as i32);
            }
            Self::Health(health) => {
                write_data_header(out, 9, SERIALIZER_FLOAT);
                write_f32(out, *health);
            }
            Self::ArmorStandFlags(flags) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::ArmorStandHeadPose(rot)
            | Self::ArmorStandBodyPose(rot)
            | Self::ArmorStandLeftArmPose(rot)
            | Self::ArmorStandRightArmPose(rot)
            | Self::ArmorStandLeftLegPose(rot)
            | Self::ArmorStandRightLegPose(rot) => {
                write_data_header(out, self.index(), SERIALIZER_ROTATIONS);
                for value in rot {
                    write_f32(out, *value);
                }
            }
            Self::EffectParticles(colors) => {
                write_data_header(out, 10, SERIALIZER_PARTICLES);
                write_varint(out, colors.len() as i32);
                for &color in colors {
                    write_varint(
                        out,
                        mc_bamboo_data::registries::particle_type::ENTITY_EFFECT,
                    );
                    out.extend_from_slice(&color.to_be_bytes());
                }
            }
            Self::EffectAmbience(ambient) => {
                write_data_header(out, 11, SERIALIZER_BOOLEAN);
                write_bool(out, *ambient);
            }
            Self::FireworksShotAtAngle(value) => {
                write_data_header(out, 10, SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::SleepingPos(pos) => {
                write_data_header(out, 14, SERIALIZER_OPTIONAL_BLOCK_POS);
                write_bool(out, pos.is_some());
                if let Some(pos) = pos {
                    write_block_pos(out, *pos);
                }
            }
            Self::MainHand(arm) => {
                write_data_header(out, 15, SERIALIZER_HUMANOID_ARM);
                write_varint(out, *arm as i32);
            }
            Self::ModelCustomisation(parts) => {
                write_data_header(out, 16, SERIALIZER_BYTE);
                out.push(*parts);
            }
            Self::AirSupply(air) => {
                write_data_header(out, 1, SERIALIZER_INT);
                write_varint(out, *air);
            }
            Self::CustomName(name) => {
                write_data_header(out, 2, SERIALIZER_OPTIONAL_COMPONENT);
                write_bool(out, true);
                name.write_network(out);
            }
            Self::NoCustomName => {
                write_data_header(out, 2, SERIALIZER_OPTIONAL_COMPONENT);
                write_bool(out, false);
            }
            Self::CustomNameVisible(visible) => {
                write_data_header(out, 3, SERIALIZER_BOOLEAN);
                write_bool(out, *visible);
            }
            Self::Silent(silent) => {
                write_data_header(out, 4, SERIALIZER_BOOLEAN);
                write_bool(out, *silent);
            }
            Self::NoGravity(no_gravity) => {
                write_data_header(out, 5, SERIALIZER_BOOLEAN);
                write_bool(out, *no_gravity);
            }
            Self::TicksFrozen(ticks) => {
                write_data_header(out, 7, SERIALIZER_INT);
                write_varint(out, *ticks);
            }
            Self::LivingFlags(flags) => {
                write_data_header(out, 8, SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::MobFlags(flags) => {
                write_data_header(out, 15, SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::Baby(baby) => {
                write_data_header(out, 16, SERIALIZER_BOOLEAN);
                write_bool(out, *baby);
            }
            Self::AgeLocked(locked) => {
                write_data_header(out, 17, SERIALIZER_BOOLEAN);
                write_bool(out, *locked);
            }
            Self::CowVariant(variant) => {
                write_data_header(out, 18, SERIALIZER_COW_VARIANT);
                write_varint(out, *variant);
            }
            // `VillagerData.STREAM_CODEC`：品种、职业两个注册表 ID 和等级，都是 VarInt。
            //
            // `VillagerData.STREAM_CODEC`: the kind and profession registry ids and
            // the level, all VarInts.
            Self::VillagerData {
                kind,
                profession,
                level,
            } => {
                write_data_header(
                    out,
                    mc_bamboo_data::villager::VILLAGER_DATA_INDEX,
                    mc_bamboo_data::villager::VILLAGER_DATA_SERIALIZER,
                );
                write_varint(out, *kind);
                write_varint(out, *profession);
                write_varint(out, *level);
            }
            Self::VillagerDataFinalized(finalized) => {
                write_data_header(
                    out,
                    mc_bamboo_data::villager::VILLAGER_DATA_FINALIZED_INDEX,
                    mc_bamboo_data::villager::VILLAGER_DATA_FINALIZED_SERIALIZER,
                );
                write_bool(out, *finalized);
            }
            Self::VillagerUnhappy(counter) => {
                write_data_header(
                    out,
                    mc_bamboo_data::villager::UNHAPPY_COUNTER_INDEX,
                    mc_bamboo_data::villager::UNHAPPY_COUNTER_SERIALIZER,
                );
                write_varint(out, *counter);
            }
            Self::CowSoundVariant(variant) => {
                write_data_header(out, 19, SERIALIZER_COW_SOUND_VARIANT);
                write_varint(out, *variant);
            }
            Self::PigBoostTime(value) | Self::PigVariant(value) | Self::PigSoundVariant(value) => {
                use mc_bamboo_data::entity_types as facts;
                let serializer = match self {
                    Self::PigVariant(_) => facts::PIG_VARIANT_SERIALIZER,
                    Self::PigSoundVariant(_) => facts::PIG_SOUND_VARIANT_SERIALIZER,
                    _ => SERIALIZER_INT,
                };
                write_data_header(out, self.index(), serializer);
                write_varint(out, *value);
            }
            Self::SheepWool(wool) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*wool);
            }
            Self::FrogVariant(variant) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::FROG_VARIANT_SERIALIZER,
                );
                write_varint(out, *variant);
            }
            // `ByteBufCodecs.OPTIONAL_VAR_INT`：空是 0，否则是值加一。
            // `ByteBufCodecs.OPTIONAL_VAR_INT`: 0 when empty, otherwise the value plus one.
            Self::FrogTongueTarget(target) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::FROG_TONGUE_TARGET_SERIALIZER,
                );
                write_varint(out, target.map_or(0, |id| id + 1));
            }
            Self::ArmadilloState(state) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::ARMADILLO_STATE_SERIALIZER,
                );
                write_varint(out, *state);
            }
            Self::CopperGolemWeatherState(state) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::COPPER_GOLEM_WEATHER_STATE_SERIALIZER,
                );
                write_varint(out, *state);
            }
            Self::CopperGolemState(state) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::COPPER_GOLEM_STATE_SERIALIZER,
                );
                write_varint(out, *state);
            }
            Self::SnifferState(state) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::SNIFFER_STATE_SERIALIZER,
                );
                write_varint(out, *state);
            }
            Self::SnifferDropSeedAtTick(tick) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *tick);
            }
            Self::ChickenVariant(value) | Self::ChickenSoundVariant(value) => {
                use mc_bamboo_data::entity_types as facts;
                let serializer = if matches!(self, Self::ChickenVariant(_)) {
                    facts::CHICKEN_VARIANT_SERIALIZER
                } else {
                    facts::CHICKEN_SOUND_VARIANT_SERIALIZER
                };
                write_data_header(out, self.index(), serializer);
                write_varint(out, *value);
            }
            Self::MooshroomType(kind) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *kind);
            }
            Self::PolarBearStanding(standing) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *standing);
            }
            Self::OcelotTrusting(trusting) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *trusting);
            }
            Self::CatVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_CAT_VARIANT);
                write_varint(out, *value);
            }
            Self::CatIsLying(value) | Self::CatRelaxStateOne(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::CatCollar(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::CatSoundVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_CAT_SOUND_VARIANT);
                write_varint(out, *value);
            }
            Self::ParrotVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::PandaUnhappy(value) | Self::PandaSneeze(value) | Self::PandaEat(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::PandaMainGene(gene)
            | Self::PandaHiddenGene(gene)
            | Self::PandaFlags(gene)
            | Self::BeeFlags(gene)
            | Self::GolemFlags(gene)
            | Self::BatFlags(gene) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*gene);
            }
            Self::GlowSquidDarkTicks(ticks) | Self::PhantomSize(ticks) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *ticks);
            }
            Self::FishFromBucket(from_bucket) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *from_bucket);
            }
            Self::TurtleHasEgg(value) | Self::TurtleLayingEgg(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::DolphinGotFish(value)
            | Self::AxolotlPlayingDead(value)
            | Self::AxolotlFromBucket(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::DolphinMoistness(value) | Self::AxolotlVariant(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::SalmonVariant(value)
            | Self::TropicalFishVariant(value)
            | Self::PufferfishPuffState(value)
            | Self::ClientAngerLevel(value) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *value);
            }
            Self::SpecialType(kind) => {
                write_data_header(out, 17, SERIALIZER_INT);
                write_varint(out, *kind);
            }
            Self::CreeperSwellDir(direction) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *direction);
            }
            Self::EndermanCarried(state) => {
                write_data_header(out, self.index(), SERIALIZER_OPTIONAL_BLOCK_STATE);
                write_varint(out, state.unwrap_or(0));
            }
            Self::CreeperPowered(flag)
            | Self::CreeperIgnited(flag)
            | Self::EndermanCreepy(flag)
            | Self::EndermanStaredAt(flag) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *flag);
            }
            Self::SpiderFlags(flags) => {
                write_data_header(out, self.index(), SERIALIZER_BYTE);
                out.push(*flags);
            }
            Self::RabbitVariant(kind) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *kind);
            }
            Self::CubeMobSize(size) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *size);
            }
            Self::SulfurCubeMaxFuse(fuse) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *fuse);
            }
            Self::SulfurCubeFromBucket(flag) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *flag);
            }
            Self::CreakingCanMove(value)
            | Self::CreakingActive(value)
            | Self::CreakingTearingDown(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::CreakingHomePos(pos) => {
                write_data_header(out, self.index(), SERIALIZER_OPTIONAL_BLOCK_POS);
                write_bool(out, pos.is_some());
                if let Some(pos) = pos {
                    write_block_pos(out, *pos);
                }
            }
            Self::DrownedConverting(converting) => {
                write_data_header(out, 18, SERIALIZER_BOOLEAN);
                write_bool(out, *converting);
            }
            Self::ZombieVillagerConverting(converting) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *converting);
            }
            Self::ZombieVillagerData {
                kind,
                profession,
                level,
            } => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::villager::VILLAGER_DATA_SERIALIZER,
                );
                write_varint(out, *kind);
                write_varint(out, *profession);
                write_varint(out, *level);
            }
            Self::ZombieVillagerDataFinalized(finalized) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *finalized);
            }
            Self::WitchUsingItem(using) => {
                write_data_header(out, 17, SERIALIZER_BOOLEAN);
                write_bool(out, *using);
            }
            Self::NautilusDash(value) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *value);
            }
            Self::ZombieNautilusVariant(variant) => {
                write_data_header(
                    out,
                    self.index(),
                    mc_bamboo_data::entity_types::ZOMBIE_NAUTILUS_VARIANT_SERIALIZER,
                );
                write_varint(out, *variant);
            }
            Self::GuardianMoving(moving) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *moving);
            }
            Self::GuardianAttackTarget(target) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *target);
            }
            Self::HappyGhastLeashHolder(flag) | Self::HappyGhastStaysStill(flag) => {
                write_data_header(out, self.index(), SERIALIZER_BOOLEAN);
                write_bool(out, *flag);
            }
            Self::HangingDirection(direction) => {
                write_data_header(out, self.index(), SERIALIZER_DIRECTION);
                write_varint(out, direction.id());
            }
            Self::ItemFrameItem(stack) => {
                write_data_header(out, self.index(), SERIALIZER_ITEM_STACK);
                stack.write(out);
            }
            Self::ItemFrameRotation(rotation) => {
                write_data_header(out, self.index(), SERIALIZER_INT);
                write_varint(out, *rotation);
            }
            Self::PaintingVariant(variant) => {
                write_data_header(out, self.index(), SERIALIZER_PAINTING_VARIANT);
                // `PaintingVariant.STREAM_CODEC` = `ByteBufCodecs.holder(PAINTING_VARIANT, DIRECT_STREAM_CODEC)`：
                // 注册表项写 `id + 1` 的 VarInt（0 留给内联定义）。
                write_varint(out, *variant + 1);
            }
        }
    }
}

fn write_data_header(out: &mut Vec<u8>, index: u8, serializer: i32) {
    out.push(index);
    write_varint(out, serializer);
}

/// 更新实体元数据，只写变化的项。配对时 index 要升序，和官方一致。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetEntityData<'a> {
    pub entity_id: i32,
    pub items: &'a [EntityData],
}

impl ClientboundPacket for SetEntityData<'_> {
    const ID: i32 = clientbound::SET_ENTITY_DATA;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        for item in self.items {
            item.write(out);
        }
        out.push(ENTITY_DATA_END);
    }
}

/// 载具和它的乘客，`ClientboundSetPassengersPacket`：载具的实体 ID 加一个 varint 数组，
/// 长度在前，按乘客的上下车顺序。空数组表示没人骑。
///
/// A vehicle and its passengers, `ClientboundSetPassengersPacket`: the vehicle's
/// entity id and a length-prefixed varint array in riding order. An empty array
/// clears the vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetPassengers<'a> {
    pub vehicle: i32,
    pub passengers: &'a [i32],
}

impl ClientboundPacket for SetPassengers<'_> {
    const ID: i32 = clientbound::SET_PASSENGERS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.vehicle);
        write_varint(out, self.passengers.len() as i32);
        for &passenger in self.passengers {
            write_varint(out, passenger);
        }
    }
}

/// 拴绳的牵引关系，`ClientboundSetEntityLinkPacket`：被拴的实体和牵引者，牵引者空时是 0。
/// 两个字段都是 4 字节定长整数（官方 `writeInt`，不是 varint）。
///
/// A leash link: the leashed entity and its holder, 0 when there is none. Both
/// fields are four-byte ints (vanilla `writeInt`, not varint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEntityLink {
    pub source_id: i32,
    pub dest_id: i32,
}

impl ClientboundPacket for SetEntityLink {
    const ID: i32 = clientbound::SET_ENTITY_LINK;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i32(out, self.source_id);
        write_i32(out, self.dest_id);
    }
}

/// 玩家捡起地上的物品，`ClientboundTakeItemEntityPacket`。
///
/// 三个 varint：物品实体、捡起它的玩家、这次拿走的数量。
///
/// A player picks up a ground item. Three varints: the item entity, the player, the amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeItemEntity {
    pub item_id: i32,
    pub player_id: i32,
    pub amount: i32,
}

impl ClientboundPacket for TakeItemEntity {
    const ID: i32 = clientbound::TAKE_ITEM_ENTITY;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.item_id);
        write_varint(out, self.player_id);
        write_varint(out, self.amount);
    }
}

/// 属性修饰符的运算方式。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeOperation {
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

/// 一个属性修饰符。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeModifier<'a> {
    pub id: &'a str,
    pub amount: f64,
    pub operation: AttributeOperation,
}

/// 一个属性的基础值和修饰符。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeSnapshot<'a> {
    /// `minecraft:attribute` 注册表 ID。
    pub attribute: i32,
    pub base: f64,
    pub modifiers: &'a [AttributeModifier<'a>],
}

/// 同步实体的属性。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpdateAttributes<'a> {
    pub entity_id: i32,
    pub attributes: &'a [AttributeSnapshot<'a>],
}

impl ClientboundPacket for UpdateAttributes<'_> {
    const ID: i32 = clientbound::UPDATE_ATTRIBUTES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.attributes.len() as i32);
        for attribute in self.attributes {
            write_varint(out, attribute.attribute);
            write_f64(out, attribute.base);
            write_varint(out, attribute.modifiers.len() as i32);
            for modifier in attribute.modifiers {
                write_string(out, modifier.id);
                write_f64(out, modifier.amount);
                write_varint(out, modifier.operation as i32);
            }
        }
    }
}

/// 实体的装备槽。枚举值是官方的 ordinal，不是 `EquipmentSlot.id`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquipmentSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

/// 更新实体的装备，发给看得到这个实体的其他玩家。
///
/// 物品带着组件：客户端要靠它们画附魔光效和改过的名字。组件原样写，和官方一样。
///
/// Updates an entity's equipment for players who can see it. Components are written
/// as stored, matching vanilla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEquipment<'a> {
    entity_id: i32,
    slots: &'a [(EquipmentSlot, &'a ItemStack)],
}

impl<'a> SetEquipment<'a> {
    /// `slots` 为空时返回 `None`：线上没有个数，客户端至少读一项，空包会让它断线。
    pub fn new(entity_id: i32, slots: &'a [(EquipmentSlot, &'a ItemStack)]) -> Option<Self> {
        (!slots.is_empty()).then_some(Self { entity_id, slots })
    }
}

impl ClientboundPacket for SetEquipment<'_> {
    const ID: i32 = clientbound::SET_EQUIPMENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        for (index, (slot, stack)) in self.slots.iter().enumerate() {
            let is_last = index + 1 == self.slots.len();
            out.push(*slot as u8 | if is_last { 0 } else { EQUIPMENT_CONTINUE });
            stack.write(out);
        }
    }
}

/// 挥手动画的类型。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingAnimationType {
    None = 0,
    Whack = 1,
    Stab = 2,
}

/// 让其他玩家看到实体挥手。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    pub entity_id: i32,
    pub hand: Hand,
    pub animation: SwingAnimationType,
    /// 动画的 tick 数。
    pub duration: i32,
}

impl SwingAnimation {
    /// 物品没有自定义动画时的挥手。
    pub const fn default_swing(entity_id: i32, hand: Hand) -> Self {
        Self {
            entity_id,
            hand,
            animation: SwingAnimationType::Whack,
            duration: 6,
        }
    }
}

impl ClientboundPacket for SwingAnimation {
    const ID: i32 = clientbound::SWING_ANIMATION;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.hand as i32);
        write_varint(out, self.animation as i32);
        write_varint(out, self.duration);
    }
}

/// 官方 `ClientboundAnimatePacket`：VarInt 实体 ID + 一个字节的动作。
/// 26.3 把挥手挪到了 `swing_animation`，这里只剩暴击的两种动作：
/// `CRITICAL_HIT = 1`、`MAGIC_CRITICAL_HIT = 2`，`WAKE_UP = 0` 不用。
///
/// Vanilla `ClientboundAnimatePacket`: a VarInt entity id and one action byte.
/// 26.3 moved swinging to `swing_animation`; only the two critical-hit actions
/// remain here: `CRITICAL_HIT = 1` and `MAGIC_CRITICAL_HIT = 2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Animate {
    pub entity_id: i32,
    pub action: u8,
}

impl Animate {
    /// `ClientboundAnimatePacket.CRITICAL_HIT`。
    pub const CRITICAL_HIT: u8 = 1;
    /// `ClientboundAnimatePacket.MAGIC_CRITICAL_HIT`。
    pub const MAGIC_CRITICAL_HIT: u8 = 2;
}

impl ClientboundPacket for Animate {
    const ID: i32 = clientbound::ANIMATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        out.push(self.action);
    }
}

/// 官方 `ClientboundUpdateMobEffectPacket`：VarInt 实体 ID、效果 Holder(VarInt)、
/// VarInt 等级、VarInt 剩余刻数、一个字节的标志位
/// （ambient 1 / visible 2 / showIcon 4 / blend 8）。
///
/// Vanilla `ClientboundUpdateMobEffectPacket`: entity id, effect holder,
/// amplifier, remaining duration and one byte of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateMobEffect {
    pub entity_id: i32,
    pub effect: i32,
    pub amplifier: i32,
    pub duration: i32,
    pub ambient: bool,
    pub visible: bool,
    pub show_icon: bool,
    pub blend: bool,
}

impl ClientboundPacket for UpdateMobEffect {
    const ID: i32 = clientbound::UPDATE_MOB_EFFECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.effect);
        write_varint(out, self.amplifier);
        write_varint(out, self.duration);
        let mut flags = 0_u8;
        if self.ambient {
            flags |= 1;
        }
        if self.visible {
            flags |= 2;
        }
        if self.show_icon {
            flags |= 4;
        }
        if self.blend {
            flags |= 8;
        }
        out.push(flags);
    }
}

/// 官方 `ClientboundRemoveMobEffectPacket`：实体 ID + 效果 Holder。
///
/// Vanilla `ClientboundRemoveMobEffectPacket`: entity id plus effect holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveMobEffect {
    pub entity_id: i32,
    pub effect: i32,
}

impl ClientboundPacket for RemoveMobEffect {
    const ID: i32 = clientbound::REMOVE_MOB_EFFECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.effect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex};

    const ENTITY: i32 = 7;

    fn set_entity_data(entity_id: i32, items: &[EntityData]) -> Vec<u8> {
        body(&SetEntityData { entity_id, items })
    }

    #[test]
    fn armour_stand_metadata_uses_the_official_indices_and_serializers() {
        assert_eq!(EntityData::ArmorStandFlags(0).index(), 15);
        assert_eq!(EntityData::ArmorStandHeadPose([0.0; 3]).index(), 16);
        assert_eq!(EntityData::ArmorStandBodyPose([0.0; 3]).index(), 17);
        assert_eq!(EntityData::ArmorStandLeftArmPose([0.0; 3]).index(), 18);
        assert_eq!(EntityData::ArmorStandRightArmPose([0.0; 3]).index(), 19);
        assert_eq!(EntityData::ArmorStandLeftLegPose([0.0; 3]).index(), 20);
        assert_eq!(EntityData::ArmorStandRightLegPose([0.0; 3]).index(), 21);

        let mut flags = Vec::new();
        EntityData::ArmorStandFlags(4).write(&mut flags);
        assert_eq!(flags, vec![15, 0, 4]);

        let mut pose = Vec::new();
        EntityData::ArmorStandHeadPose([1.0, 2.0, 3.0]).write(&mut pose);
        assert_eq!(
            pose,
            vec![
                16, 9, 0x3f, 0x80, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x40, 0x40, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn add_entity_matches_official_bytes() {
        let packet = AddEntity {
            entity_id: ENTITY,
            uuid: Uuid::from_u128(0x069a79f4_44e9_4726_a5be_fca90e38aaf5),
            entity_type: 159,
            position: Vec3::new(8.5, 65.0, -3.25),
            movement: Vec3::default(),
            pitch: 0x00,
            yaw: 0x40,
            head_yaw: 0x40,
            data: 0,
        };
        assert_eq!(
            body(&packet),
            hex("07
                 06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5
                 9f 01
                 40 21 00 00 00 00 00 00
                 40 50 40 00 00 00 00 00
                 c0 0a 00 00 00 00 00 00
                 00
                 00 40 40
                 00")
        );
    }

    #[test]
    fn remove_entities_lists_var_ints() {
        let packet = RemoveEntities {
            entity_ids: &[7, 300],
        };
        assert_eq!(body(&packet), hex("02 07 ac 02"));
    }

    #[test]
    fn set_entity_link_writes_two_four_byte_ints() {
        assert_eq!(
            body(&SetEntityLink {
                source_id: 7,
                dest_id: 300,
            }),
            hex("00 00 00 07 00 00 01 2c")
        );
        // 牵引者空时 dest_id 是 0。
        assert_eq!(
            body(&SetEntityLink {
                source_id: 7,
                dest_id: 0,
            }),
            hex("00 00 00 07 00 00 00 00")
        );
    }

    #[test]
    fn set_entity_data_matches_official_bytes() {
        let items = [
            EntityData::SharedFlags(ENTITY_FLAG_SHIFT_KEY_DOWN | ENTITY_FLAG_SPRINTING),
            EntityData::Pose(Pose::Crouching),
            EntityData::Health(20.0),
            EntityData::MainHand(HumanoidArm::Left),
            EntityData::ModelCustomisation(0x7f),
        ];
        assert_eq!(
            set_entity_data(ENTITY, &items),
            hex("07
                 00 00 0a
                 06 14 05
                 09 03 41 a0 00 00
                 0f 2a 00
                 10 00 7f
                 ff")
        );
    }

    #[test]
    fn world_entity_data_uses_their_own_indices() {
        // 下落的方块：index 8 的 BLOCK_POS，打包成 long。
        assert_eq!(
            set_entity_data(
                ENTITY,
                &[EntityData::FallingBlockStart(BlockPos::new(1, 2, 3))]
            ),
            hex("07 08 0a 00 00 00 40 00 00 30 02 ff")
        );
        // TNT 的引信和起点都占 index 8，靠序列化器区分。
        assert_eq!(
            set_entity_data(ENTITY, &[EntityData::TntFuse(80)]),
            hex("07 08 01 50 ff")
        );
        // 方块状态的 index 9 和玩家的生命值一样，同样靠序列化器区分。
        assert_eq!(
            set_entity_data(ENTITY, &[EntityData::TntBlockState(300)]),
            hex("07 09 0e ac 02 ff")
        );
    }

    /// 嗅探兽的两项元数据按官方下标和序列化器编码：`DATA_STATE` 是 index 18、
    /// 35 号 `SNIFFER_STATE` 序列化器（VarInt 写状态 id），`DATA_DROP_SEED_AT_TICK`
    /// 是 index 19、INT 序列化器。数字从官方 jar 的反射探针导出。
    ///
    /// The sniffer's two metadata items use the vanilla indices and serializers:
    /// `DATA_STATE` is index 18 with serializer 35 (`SNIFFER_STATE`, a VarInt state
    /// id) and `DATA_DROP_SEED_AT_TICK` is index 19 with the INT serializer. The
    /// numbers come from a reflection probe over the official jar.
    #[test]
    fn sniffer_metadata_matches_official_indices_and_serializers() {
        use mc_bamboo_data::entity_types as facts;
        assert_eq!(facts::SNIFFER_STATE, 18);
        assert_eq!(facts::SNIFFER_DROP_SEED_AT_TICK, 19);
        assert_eq!(facts::SNIFFER_STATE_SERIALIZER, 35);
        assert_eq!(EntityData::SnifferState(5).index(), 18);
        assert_eq!(EntityData::SnifferDropSeedAtTick(120).index(), 19);
        // entity 7、index 18 / 35 号序列化器 / DIGGING 状态 5，最后 0xff 收尾。
        // Entity 7, index 18 / serializer 35 / state DIGGING (5), then the 0xff end.
        assert_eq!(
            set_entity_data(ENTITY, &[EntityData::SnifferState(5)]),
            hex("07 12 23 05 ff")
        );
        // entity 7、index 19 / INT 序列化器 1 / tick 120，最后 0xff 收尾。
        // Entity 7, index 19 / INT serializer 1 / tick 120, then the 0xff end.
        assert_eq!(
            set_entity_data(ENTITY, &[EntityData::SnifferDropSeedAtTick(120)]),
            hex("07 13 01 78 ff")
        );
        // 两项一起，按 index 升序。
        // Both together, in ascending index order.
        assert_eq!(
            set_entity_data(
                ENTITY,
                &[
                    EntityData::SnifferState(5),
                    EntityData::SnifferDropSeedAtTick(120),
                ]
            ),
            hex("07 12 23 05 13 01 78 ff")
        );
    }

    #[test]
    fn main_hand_and_model_parts_match_the_join_example() {
        let items = [
            EntityData::MainHand(HumanoidArm::Left),
            EntityData::ModelCustomisation(0x7f),
        ];
        assert_eq!(set_entity_data(1, &items), hex("01 0f 2a 00 10 00 7f ff"));
    }

    #[test]
    fn fall_flying_refusal_matches_official_bytes() {
        let packet = SetEntityData {
            entity_id: ENTITY,
            items: &[EntityData::SharedFlags(0)],
        };
        assert_eq!(body(&packet), hex("07 00 00 00 ff"));
        assert_eq!(frame(&packet), hex("06 65 07 00 00 00 ff"));

        let sprinting = [EntityData::SharedFlags(ENTITY_FLAG_SPRINTING)];
        assert_eq!(set_entity_data(ENTITY, &sprinting), hex("07 00 00 08 ff"));
    }

    #[test]
    fn update_attributes_matches_official_bytes() {
        let block_range = [AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }];
        let entity_range = [AttributeModifier {
            id: "minecraft:creative_mode_entity_range",
            amount: 2.0,
            operation: AttributeOperation::AddValue,
        }];
        let packet = UpdateAttributes {
            entity_id: 1,
            attributes: &[
                AttributeSnapshot {
                    attribute: 8,
                    base: 4.5,
                    modifiers: &block_range,
                },
                AttributeSnapshot {
                    attribute: 13,
                    base: 3.0,
                    modifiers: &entity_range,
                },
            ],
        };

        let mut expected = hex("01 02 08 40 12 00 00 00 00 00 00 01 23");
        expected.extend_from_slice(b"minecraft:creative_mode_block_range");
        expected.extend(hex("3f e0 00 00 00 00 00 00 00"));
        expected.extend(hex("0d 40 08 00 00 00 00 00 00 01 24"));
        expected.extend_from_slice(b"minecraft:creative_mode_entity_range");
        expected.extend(hex("40 00 00 00 00 00 00 00 00"));

        assert_eq!(expected.len(), 113);
        assert_eq!(body(&packet), expected);
        // ID 大于 127 时占两个字节。
        assert_eq!(frame(&packet)[1..3], [0x86, 0x01]);
    }

    #[test]
    fn update_attributes_matches_the_single_attribute_examples() {
        let modifiers = [AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }];
        let attributes = [AttributeSnapshot {
            attribute: 8,
            base: 4.5,
            modifiers: &modifiers,
        }];

        for (entity_id, entity_hex) in [(ENTITY, "07"), (5, "05")] {
            let mut expected = hex(entity_hex);
            expected.extend(hex("01 08 40 12 00 00 00 00 00 00 01 23"));
            expected.extend_from_slice(b"minecraft:creative_mode_block_range");
            expected.extend(hex("3f e0 00 00 00 00 00 00 00"));

            let packet = UpdateAttributes {
                entity_id,
                attributes: &attributes,
            };
            assert_eq!(body(&packet), expected);
        }
    }

    #[test]
    fn set_equipment_needs_at_least_one_slot() {
        assert_eq!(SetEquipment::new(ENTITY, &[]), None);
    }

    #[test]
    fn set_equipment_marks_every_slot_but_the_last() {
        let stone = ItemStack::new(1, 1);
        let empty = ItemStack::empty();
        let slots = [
            (EquipmentSlot::MainHand, &stone),
            (EquipmentSlot::OffHand, &empty),
        ];
        let packet = SetEquipment::new(5, &slots).unwrap();
        assert_eq!(body(&packet), hex("05 80 01 01 00 00 01 00"));
    }

    #[test]
    fn set_equipment_matches_the_three_slot_example() {
        let stone = ItemStack::new(1, 64);
        let empty = ItemStack::empty();
        let helmet = ItemStack::new(1084, 1);
        let slots = [
            (EquipmentSlot::MainHand, &stone),
            (EquipmentSlot::OffHand, &empty),
            (EquipmentSlot::Head, &helmet),
        ];
        let packet = SetEquipment::new(ENTITY, &slots).unwrap();
        assert_eq!(
            body(&packet),
            hex("07
                 80 40 01 00 00
                 81 00
                 05 01 bc 08 00 00")
        );
    }

    #[test]
    fn set_equipment_forwards_the_components() {
        let stack = ItemStack {
            item: 1,
            count: 2,
            added: vec![(1, vec![0x10])],
            removed: vec![12],
        };
        let slots = [(EquipmentSlot::MainHand, &stack)];
        let packet = SetEquipment::new(ENTITY, &slots).unwrap();
        assert_eq!(body(&packet), hex("07 00 02 01 01 01 01 10 0c"));
    }

    #[test]
    fn swing_animation_matches_official_bytes() {
        let main_hand = SwingAnimation::default_swing(ENTITY, Hand::Main);
        assert_eq!(body(&main_hand), hex("07 00 01 06"));
        assert_eq!(frame(&main_hand), hex("05 7b 07 00 01 06"));

        let stab = SwingAnimation {
            entity_id: ENTITY,
            hand: Hand::Off,
            animation: SwingAnimationType::Stab,
            duration: 10,
        };
        assert_eq!(body(&stab), hex("07 01 02 0a"));
        assert_eq!(
            body(&SwingAnimation::default_swing(5, Hand::Main)),
            hex("05 00 01 06")
        );
    }

    #[test]
    fn animate_packet_matches_official_bytes() {
        let crit = Animate {
            entity_id: ENTITY,
            action: Animate::CRITICAL_HIT,
        };
        assert_eq!(body(&crit), hex("07 01"));
        assert_eq!(frame(&crit), hex("03 02 07 01"));

        let magic = Animate {
            entity_id: ENTITY,
            action: Animate::MAGIC_CRITICAL_HIT,
        };
        assert_eq!(body(&magic), hex("07 02"));
    }

    #[test]
    fn effect_packets_match_the_official_layout() {
        // `ClientboundUpdateMobEffectPacket.write`: VarInt 实体 ID / 效果 /
        // 等级 / 时长 / flags。flags = ambient|visible|showIcon|blend = 1|2|4|8。
        let added = UpdateMobEffect {
            entity_id: 7,
            effect: 9,
            amplifier: 2,
            duration: 600,
            ambient: true,
            visible: true,
            show_icon: true,
            blend: true,
        };
        assert_eq!(frame(&added), hex("08 87 01 07 09 02 d8 04 0f"));
        let updated = UpdateMobEffect {
            blend: false,
            ambient: false,
            visible: false,
            show_icon: false,
            ..added
        };
        assert_eq!(body(&updated), hex("07 09 02 d8 04 00"));
        let removed = RemoveMobEffect {
            entity_id: 7,
            effect: 9,
        };
        assert_eq!(frame(&removed), hex("03 4f 07 09"));
    }

    #[test]
    fn effect_metadata_uses_the_particles_and_boolean_serializers() {
        let particles = EntityData::EffectParticles(vec![3_402_751, 9_154_528]);
        assert_eq!(particles.index(), 10);
        let mut expected = hex("07 0a 11 02 1c");
        expected.extend_from_slice(&3_402_751_i32.to_be_bytes());
        expected.push(28);
        expected.extend_from_slice(&9_154_528_i32.to_be_bytes());
        expected.push(0xff);
        assert_eq!(
            body(&SetEntityData {
                entity_id: ENTITY,
                items: &[particles],
            }),
            expected
        );
        let ambient = EntityData::EffectAmbience(true);
        assert_eq!(ambient.index(), 11);
        assert_eq!(
            body(&SetEntityData {
                entity_id: ENTITY,
                items: &[ambient],
            }),
            hex("07 0b 08 01 ff")
        );
    }

    #[test]
    fn trident_loyalty_and_foil_metadata_match_official_indices() {
        let loyalty = EntityData::TridentLoyalty(3);
        let foil = EntityData::TridentFoil(true);
        assert_eq!(
            loyalty.index(),
            mc_bamboo_data::entity_types::TRIDENT_LOYALTY
        );
        assert_eq!(foil.index(), mc_bamboo_data::entity_types::TRIDENT_FOIL);
        // 11 号 BYTE=0 值 3，12 号 BOOLEAN=8 值 1，最后 0xff 收尾。
        // index 11 BYTE=0 value 3, index 12 BOOLEAN=8 value 1, then the 0xff end marker.
        assert_eq!(
            set_entity_data(7, &[loyalty, foil]),
            hex("07 0b 00 03 0c 08 01 ff")
        );
    }

    #[test]
    fn hanging_metadata_matches_official_serializers_and_indices() {
        // index 8 DIRECTION=12 (0x0c) value North=2; index 9 PAINTING_VARIANT=34 (0x22) value kebab=24 -> 25 (0x19)
        let dir = EntityData::HangingDirection(Direction::North);
        let variant = EntityData::PaintingVariant(24);
        assert_eq!(dir.index(), 8);
        assert_eq!(variant.index(), 9);
        assert_eq!(
            set_entity_data(7, &[dir, variant]),
            hex("07 08 0c 02 09 22 19 ff")
        );
        // index 9 ITEM_STACK=7 (empty -> count 0), index 10 INT=1 value 3
        let item = EntityData::ItemFrameItem(ItemStack::empty());
        let rot = EntityData::ItemFrameRotation(3);
        assert_eq!(item.index(), 9);
        assert_eq!(rot.index(), 10);
        assert_eq!(
            set_entity_data(7, &[item, rot]),
            hex("07 09 07 00 0a 01 03 ff")
        );
    }
}
