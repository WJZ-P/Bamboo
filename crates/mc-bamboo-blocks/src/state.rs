//! 方块状态和属性，对应官方的 `BlockState`、`StateHolder`、`BlockStateProperties`。
//!
//! 状态 ID 是属性值的笛卡尔积，最后一个属性变化最快（生成器校验过），所以取值和改值都是算术。
//! 方块放置不是热路径，属性按名字线性查找，不做缓存。

use std::{marker::PhantomData, sync::LazyLock};

use mc_bamboo_core::{Axis, Direction};
use mc_bamboo_data::{
    Block, BlockAncestor, BlockClass, BlockTag, FluidFacts, FluidTag, ItemTag, PropertyDef, blocks,
    facts, items, registries, state_facts, state_fluid, tags,
};

/// `getLightDampeningInto` 里形状完全挡住光时的值，比最大的 15 还大。
const BLOCKED_LIGHT: i32 = 16;

/// 一个方块状态，内容是全局状态 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState(pub u32);

impl BlockState {
    pub const AIR: Self = Self(blocks::AIR);
    pub const WATER: Self = Self(blocks::WATER);

    /// 这个状态所属的方块。
    pub fn block(self) -> &'static Block {
        &blocks::BLOCKS[self.block_id()]
    }

    /// 方块的注册表 ID。
    pub fn block_id(self) -> usize {
        mc_bamboo_data::block_id_of_state(self.0).expect("BlockState 里是合法的状态 ID")
    }

    /// 官方类名。
    pub fn class(self) -> BlockClass {
        self.block().class
    }

    /// `instanceof`。
    pub fn is_a(self, ancestor: BlockAncestor) -> bool {
        self.class().ancestors().contains(&ancestor)
    }

    /// 和 `default_state_of_block` 是同一个方块。参数是 `blocks::STONE` 这样的常量。
    pub fn is(self, default_state_of_block: u32) -> bool {
        self.is_same_block(Self(default_state_of_block))
    }

    /// 和 `other` 是同一个方块，状态可以不同。
    pub fn is_same_block(self, other: Self) -> bool {
        self.block_id() == other.block_id()
    }

    /// 这个方块的默认状态。
    pub fn default_state(self) -> Self {
        Self(self.block().default_state)
    }

    /// 方块在这个标签里。
    pub fn is_tagged(self, tag: BlockTag) -> bool {
        BLOCK_TAGS[tag.index].contains(self.block_id())
    }

    /// 取属性的值。方块没有这个属性时 panic，和官方的 `getValue` 一样。
    pub fn get<T: PropertyValue>(self, property: Property<T>) -> T {
        self.try_get(property)
            .unwrap_or_else(|| panic!("{} has no property {}", self.block().name, property.name))
    }

    /// 取属性的值，方块没有这个属性时返回 `None`。
    pub fn try_get<T: PropertyValue>(self, property: Property<T>) -> Option<T> {
        let slot = PropertySlot::find(self, property.name)?;
        T::from_name(slot.def.values[slot.current_index(self)])
    }

    /// 方块有这个属性。
    pub fn has<T: PropertyValue>(self, property: Property<T>) -> bool {
        self.try_get(property).is_some()
    }

    /// 改属性的值。方块没有这个属性或者没有这个值时 panic，和官方的 `setValue` 一样。
    #[must_use]
    pub fn with<T: PropertyValue>(self, property: Property<T>, value: T) -> Self {
        self.try_with(property, value).unwrap_or_else(|| {
            panic!(
                "{} cannot set {} to this value",
                self.block().name,
                property.name
            )
        })
    }

    /// 方块有这个属性和这个值时改值，否则返回 `None`。
    #[must_use]
    pub fn try_with<T: PropertyValue>(self, property: Property<T>, value: T) -> Option<Self> {
        let slot = PropertySlot::find(self, property.name)?;
        let target = slot
            .def
            .values
            .iter()
            .position(|&text| value.is_named(text))?;
        Some(slot.with_index(self, target))
    }

    /// 换成属性的下一个值，到头后回到第一个。官方的 `cycle`。
    #[must_use]
    pub fn cycle<T: PropertyValue>(self, property: Property<T>) -> Self {
        let slot = PropertySlot::find(self, property.name)
            .unwrap_or_else(|| panic!("{} has no property {}", self.block().name, property.name));
        let next = (slot.current_index(self) + 1) % slot.def.values.len();
        slot.with_index(self, next)
    }

    /// `Block.withPropertiesOf`：`target` 的默认状态上，复制 `self` 里两边都有的属性。
    #[must_use]
    pub fn copied_onto(self, target: &Block) -> Self {
        Self(target.default_state).with_properties_of(self)
    }

    /// `BlockState.withPropertiesOf`：复制 `source` 里两边都有的属性。
    #[must_use]
    pub fn with_properties_of(self, source: Self) -> Self {
        let mut result = self;
        for def in source.block().properties {
            let Some(slot) = PropertySlot::find(source, def.name) else {
                continue;
            };
            let name = def.values[slot.current_index(source)];
            let Some(target_slot) = PropertySlot::find(result, def.name) else {
                continue;
            };
            if let Some(index) = target_slot.def.values.iter().position(|&text| text == name) {
                result = target_slot.with_index(result, index);
            }
        }
        result
    }

    fn fact(self, bit: u32) -> bool {
        state_facts(self.0) & bit != 0
    }

    /// `isAir`。
    pub fn is_air(self) -> bool {
        self.fact(facts::AIR)
    }

    /// `isFaceSturdy`，按 `BlockPos.ZERO` 读取的结果。
    ///
    /// 竹子这类形状跟着渲染偏移走的方块，结果和位置有关，判定世界里的方块时用
    /// [`World::is_face_sturdy`](crate::World::is_face_sturdy)。
    pub fn is_face_sturdy(self, face: Direction, support: SupportType) -> bool {
        self.fact(1 << (support.first_bit() + face.id() as u32))
    }

    /// `canOcclude`。
    pub fn can_occlude(self) -> bool {
        self.fact(facts::CAN_OCCLUDE)
    }

    /// `isSolid`，官方标了 `@Deprecated` 的那个粗略判定。
    pub fn is_solid(self) -> bool {
        self.fact(facts::SOLID)
    }

    /// `liquid`，方块属性里的标记，含水方块不算。
    pub fn is_liquid(self) -> bool {
        self.fact(facts::LIQUID)
    }

    /// 不带上下文的 `canBeReplaced()`，也就是 `Properties.replaceable`。
    pub fn is_replaceable(self) -> bool {
        self.fact(facts::REPLACEABLE)
    }

    /// `isRedstoneConductor`。
    pub fn is_redstone_conductor(self) -> bool {
        self.fact(facts::REDSTONE_CONDUCTOR)
    }

    /// `isCollisionShapeFullBlock`。
    pub fn is_collision_shape_full_block(self) -> bool {
        self.fact(facts::COLLISION_FULL_BLOCK)
    }

    /// `isSolidRender`。
    pub fn is_solid_render(self) -> bool {
        self.fact(facts::SOLID_RENDER)
    }

    /// `isSignalSource`。
    pub fn is_signal_source(self) -> bool {
        self.fact(facts::SIGNAL_SOURCE)
    }

    /// `hasAnalogOutputSignal`。
    pub fn has_analog_output_signal(self) -> bool {
        self.fact(facts::ANALOG_OUTPUT)
    }

    /// `propagatesSkylightDown`。
    pub fn propagates_skylight_down(self) -> bool {
        self.fact(facts::PROPAGATES_SKYLIGHT)
    }

    /// `getLightDampening`。
    pub fn light_dampening(self) -> u8 {
        mc_bamboo_data::light_dampening(self.0)
    }

    /// `isRandomlyTicking`。
    pub fn is_randomly_ticking(self) -> bool {
        self.fact(facts::RANDOMLY_TICKING)
    }

    /// `getFluidState().isRandomlyTicking()`。
    pub fn is_fluid_randomly_ticking(self) -> bool {
        self.fact(facts::FLUID_RANDOMLY_TICKING)
    }

    /// `LightEngine.getLightDampeningInto(below, self, UP, self.getLightDampening())`，`below` 是一个不用形状挡光的
    /// 完整方块（草方块、菌岩都是）。
    pub fn light_dampening_from_plain_block_below(self) -> i32 {
        if self.fact(facts::BLOCKS_LIGHT_FROM_BELOW) {
            BLOCKED_LIGHT
        } else {
            i32::from(self.light_dampening())
        }
    }

    /// `hasBlockEntity`。
    pub fn has_block_entity(self) -> bool {
        self.block().block_entity_type.is_some()
    }

    /// `instrument()`，这个方块放在音符盒下面时的乐器。
    pub fn instrument(self) -> NoteBlockInstrument {
        NoteBlockInstrument::from_name(self.block().instrument)
            .expect("生成的乐器名都在枚举里，测试校验过")
    }

    /// `getFluidState`。
    pub fn fluid(self) -> FluidState {
        FluidState(state_fluid(self.0))
    }
}

/// 一个属性在方块状态里的位置。
struct PropertySlot {
    def: &'static PropertyDef,
    /// 这个属性的值加一，状态 ID 加多少。
    stride: u32,
}

impl PropertySlot {
    fn find(state: BlockState, name: &str) -> Option<Self> {
        let block = state.block();
        let position = block
            .properties
            .iter()
            .position(|property| property.name == name)?;
        let stride = block.properties[position + 1..]
            .iter()
            .map(|property| property.values.len() as u32)
            .product();
        Some(Self {
            def: &block.properties[position],
            stride,
        })
    }

    fn current_index(&self, state: BlockState) -> usize {
        let offset = state.0 - state.block().first_state;
        (offset / self.stride) as usize % self.def.values.len()
    }

    fn with_index(&self, state: BlockState, index: usize) -> BlockState {
        let current = self.current_index(state) as u32;
        BlockState(state.0 - current * self.stride + index as u32 * self.stride)
    }
}

/// 一个属性：名字和值的类型。常量的名字和官方 `BlockStateProperties` 对应。
///
/// 官方有同名不同类型的属性，比如 `half` 既是门的上下半，也是楼梯的上下半。
/// 这里靠类型区分，用错时 [`BlockState::try_get`] 解析不了，返回 `None`。
pub struct Property<T> {
    pub name: &'static str,
    value: PhantomData<fn() -> T>,
}

impl<T> Property<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            value: PhantomData,
        }
    }
}

impl<T> Clone for Property<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Property<T> {}

/// 属性值的类型：Rust 枚举、`bool` 或 `i32`，和官方的序列化名互转。
pub trait PropertyValue: Copy {
    /// 官方的序列化名对应的值，不认识时返回 `None`。
    fn from_name(name: &str) -> Option<Self>;
    /// 这个值的官方序列化名是不是 `name`。
    fn is_named(self, name: &str) -> bool;
}

impl PropertyValue for bool {
    fn from_name(name: &str) -> Option<Self> {
        name.parse().ok()
    }

    fn is_named(self, name: &str) -> bool {
        Self::from_name(name) == Some(self)
    }
}

impl PropertyValue for i32 {
    fn from_name(name: &str) -> Option<Self> {
        name.parse().ok()
    }

    fn is_named(self, name: &str) -> bool {
        Self::from_name(name) == Some(self)
    }
}

impl PropertyValue for Direction {
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| direction.name() == name)
    }

    fn is_named(self, name: &str) -> bool {
        self.name() == name
    }
}

impl PropertyValue for Axis {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "z" => Some(Self::Z),
            _ => None,
        }
    }

    fn is_named(self, name: &str) -> bool {
        Self::from_name(name) == Some(self)
    }
}

/// 定义一个官方的属性枚举，变体后面是序列化名。
macro_rules! property_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// 官方的序列化名。
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl PropertyValue for $name {
            fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }

            fn is_named(self, name: &str) -> bool {
                self.name() == name
            }
        }
    };
}

property_enum!(
    /// `AttachFace`。
    AttachFace {
        Floor => "floor",
        Wall => "wall",
        Ceiling => "ceiling",
    }
);
property_enum!(
    /// `BellAttachType`。
    BellAttachType {
        Floor => "floor",
        Ceiling => "ceiling",
        SingleWall => "single_wall",
        DoubleWall => "double_wall",
    }
);
property_enum!(
    /// `WallSide`。
    WallSide {
        None => "none",
        Low => "low",
        Tall => "tall",
    }
);
property_enum!(
    /// `RedstoneSide`。
    RedstoneSide {
        Up => "up",
        Side => "side",
        None => "none",
    }
);
property_enum!(
    /// `DoubleBlockHalf`。
    DoubleBlockHalf {
        Upper => "upper",
        Lower => "lower",
    }
);
property_enum!(
    /// `Half`。
    Half {
        Top => "top",
        Bottom => "bottom",
    }
);
property_enum!(
    /// `SideChainPart`。
    SideChainPart {
        Unconnected => "unconnected",
        Right => "right",
        Center => "center",
        Left => "left",
    }
);
property_enum!(
    /// `RailShape`。
    RailShape {
        NorthSouth => "north_south",
        EastWest => "east_west",
        AscendingEast => "ascending_east",
        AscendingWest => "ascending_west",
        AscendingNorth => "ascending_north",
        AscendingSouth => "ascending_south",
        SouthEast => "south_east",
        SouthWest => "south_west",
        NorthWest => "north_west",
        NorthEast => "north_east",
    }
);
property_enum!(
    /// `BedPart`。
    BedPart {
        Head => "head",
        Foot => "foot",
    }
);
property_enum!(
    /// `ChestType`。
    ChestType {
        Single => "single",
        Left => "left",
        Right => "right",
    }
);
property_enum!(
    /// `ComparatorMode`。
    ComparatorMode {
        Compare => "compare",
        Subtract => "subtract",
    }
);
property_enum!(
    /// `DoorHingeSide`。
    DoorHingeSide {
        Left => "left",
        Right => "right",
    }
);
property_enum!(
    /// `NoteBlockInstrument`。
    NoteBlockInstrument {
        Harp => "harp",
        Basedrum => "basedrum",
        Snare => "snare",
        Hat => "hat",
        Bass => "bass",
        Flute => "flute",
        Bell => "bell",
        Guitar => "guitar",
        Chime => "chime",
        Xylophone => "xylophone",
        IronXylophone => "iron_xylophone",
        CowBell => "cow_bell",
        Didgeridoo => "didgeridoo",
        Bit => "bit",
        Banjo => "banjo",
        Pling => "pling",
        Trumpet => "trumpet",
        TrumpetExposed => "trumpet_exposed",
        TrumpetOxidized => "trumpet_oxidized",
        TrumpetWeathered => "trumpet_weathered",
        Zombie => "zombie",
        Skeleton => "skeleton",
        Creeper => "creeper",
        Dragon => "dragon",
        WitherSkeleton => "wither_skeleton",
        Piglin => "piglin",
        CustomHead => "custom_head",
    }
);
property_enum!(
    /// `PistonType`。
    PistonType {
        Normal => "normal",
        Sticky => "sticky",
    }
);
property_enum!(
    /// `SlabType`。
    SlabType {
        Top => "top",
        Bottom => "bottom",
        Double => "double",
    }
);
property_enum!(
    /// `StairsShape`。
    StairsShape {
        Straight => "straight",
        InnerLeft => "inner_left",
        InnerRight => "inner_right",
        OuterLeft => "outer_left",
        OuterRight => "outer_right",
    }
);
property_enum!(
    /// `StructureMode`。
    StructureMode {
        Save => "save",
        Load => "load",
        Corner => "corner",
        Data => "data",
    }
);
property_enum!(
    /// `TestBlockMode`。
    TestBlockMode {
        Start => "start",
        Log => "log",
        Fail => "fail",
        Accept => "accept",
    }
);
property_enum!(
    /// `BambooLeaves`。
    BambooLeaves {
        None => "none",
        Small => "small",
        Large => "large",
    }
);
property_enum!(
    /// `Tilt`。
    Tilt {
        None => "none",
        Unstable => "unstable",
        Partial => "partial",
        Full => "full",
    }
);
property_enum!(
    /// `SpeleothemThickness`。
    SpeleothemThickness {
        TipMerge => "tip_merge",
        Tip => "tip",
        Frustum => "frustum",
        Middle => "middle",
        Base => "base",
    }
);
property_enum!(
    /// `SculkSensorPhase`。
    SculkSensorPhase {
        Inactive => "inactive",
        Active => "active",
        Cooldown => "cooldown",
    }
);
property_enum!(
    /// `TrialSpawnerState`。
    TrialSpawnerState {
        Inactive => "inactive",
        WaitingForPlayers => "waiting_for_players",
        Active => "active",
        WaitingForRewardEjection => "waiting_for_reward_ejection",
        EjectingReward => "ejecting_reward",
        Cooldown => "cooldown",
    }
);
property_enum!(
    /// `VaultState`。
    VaultState {
        Inactive => "inactive",
        Active => "active",
        Unlocking => "unlocking",
        Ejecting => "ejecting",
    }
);
property_enum!(
    /// `CreakingHeartState`。
    CreakingHeartState {
        Uprooted => "uprooted",
        Dormant => "dormant",
        Awake => "awake",
    }
);
property_enum!(
    /// `CopperGolemStatueBlock.Pose`。
    CopperGolemPose {
        Standing => "standing",
        Sitting => "sitting",
        Running => "running",
        Star => "star",
    }
);
property_enum!(
    /// `PotentSulfurState`。
    PotentSulfurState {
        Dry => "dry",
        Wet => "wet",
        Dormant => "dormant",
        Erupting => "erupting",
        Continuous => "continuous",
    }
);
property_enum!(
    /// `FrontAndTop`：`orientation` 属性的值。
    FrontAndTop {
        DownEast => "down_east",
        DownNorth => "down_north",
        DownSouth => "down_south",
        DownWest => "down_west",
        UpEast => "up_east",
        UpNorth => "up_north",
        UpSouth => "up_south",
        UpWest => "up_west",
        WestUp => "west_up",
        EastUp => "east_up",
        NorthUp => "north_up",
        SouthUp => "south_up",
    }
);

/// 定义属性常量。
macro_rules! properties {
    ($kind:ty { $($constant:ident => $name:literal,)+ }) => {
        $(pub const $constant: Property<$kind> = Property::new($name);)+
    };
}

properties!(bool {
    ATTACHED => "attached",
    BERRIES => "berries",
    BLOOM => "bloom",
    BOTTOM => "bottom",
    CAN_SUMMON => "can_summon",
    CONDITIONAL => "conditional",
    DISARMED => "disarmed",
    DRAG => "drag",
    ENABLED => "enabled",
    EXTENDED => "extended",
    EYE => "eye",
    FALLING => "falling",
    HANGING => "hanging",
    HAS_BOTTLE_0 => "has_bottle_0",
    HAS_BOTTLE_1 => "has_bottle_1",
    HAS_BOTTLE_2 => "has_bottle_2",
    HAS_RECORD => "has_record",
    HAS_BOOK => "has_book",
    INVERTED => "inverted",
    IN_WALL => "in_wall",
    LIT => "lit",
    LOCKED => "locked",
    NATURAL => "natural",
    OCCUPIED => "occupied",
    OPEN => "open",
    PERSISTENT => "persistent",
    POWERED => "powered",
    SHORT => "short",
    SHRIEKING => "shrieking",
    SIGNAL_FIRE => "signal_fire",
    SNOWY => "snowy",
    TIP => "tip",
    TRIGGERED => "triggered",
    UNSTABLE => "unstable",
    WATERLOGGED => "waterlogged",
    UP => "up",
    DOWN => "down",
    NORTH => "north",
    EAST => "east",
    SOUTH => "south",
    WEST => "west",
    SLOT_0_OCCUPIED => "slot_0_occupied",
    SLOT_1_OCCUPIED => "slot_1_occupied",
    SLOT_2_OCCUPIED => "slot_2_occupied",
    SLOT_3_OCCUPIED => "slot_3_occupied",
    SLOT_4_OCCUPIED => "slot_4_occupied",
    SLOT_5_OCCUPIED => "slot_5_occupied",
    CRACKED => "cracked",
    CRAFTING => "crafting",
    OMINOUS => "ominous",
    MAP => "map",
});

properties!(i32 {
    FLOWER_AMOUNT => "flower_amount",
    SEGMENT_AMOUNT => "segment_amount",
    AGE_1 => "age",
    AGE_2 => "age",
    AGE_3 => "age",
    AGE_4 => "age",
    AGE_5 => "age",
    AGE_7 => "age",
    AGE_15 => "age",
    AGE_25 => "age",
    BITES => "bites",
    CANDLES => "candles",
    DELAY => "delay",
    DISTANCE => "distance",
    EGGS => "eggs",
    HATCH => "hatch",
    LAYERS => "layers",
    LEVEL_CAULDRON => "level",
    LEVEL_COMPOSTER => "level",
    LEVEL_FLOWING => "level",
    LEVEL_HONEY => "honey_level",
    LEVEL => "level",
    MOISTURE => "moisture",
    NOTE => "note",
    PICKLES => "pickles",
    POWER => "power",
    STAGE => "stage",
    STABILITY_DISTANCE => "distance",
    RESPAWN_ANCHOR_CHARGES => "charges",
    DRIED_GHAST_HYDRATION_LEVELS => "hydration",
    ROTATION_16 => "rotation",
    DUSTED => "dusted",
});

properties!(Axis {
    HORIZONTAL_AXIS => "axis",
    AXIS => "axis",
});

properties!(Direction {
    FACING => "facing",
    FACING_HOPPER => "facing",
    HORIZONTAL_FACING => "facing",
    VERTICAL_DIRECTION => "vertical_direction",
});

properties!(FrontAndTop { ORIENTATION => "orientation", });
properties!(AttachFace { ATTACH_FACE => "face", });
properties!(BellAttachType { BELL_ATTACHMENT => "attachment", });
properties!(WallSide {
    EAST_WALL => "east",
    NORTH_WALL => "north",
    SOUTH_WALL => "south",
    WEST_WALL => "west",
});
properties!(RedstoneSide {
    EAST_REDSTONE => "east",
    NORTH_REDSTONE => "north",
    SOUTH_REDSTONE => "south",
    WEST_REDSTONE => "west",
});
properties!(DoubleBlockHalf { DOUBLE_BLOCK_HALF => "half", });
properties!(Half { HALF => "half", });
properties!(SideChainPart { SIDE_CHAIN_PART => "side_chain", });
properties!(RailShape {
    RAIL_SHAPE => "shape",
    RAIL_SHAPE_STRAIGHT => "shape",
});
properties!(BedPart { BED_PART => "part", });
properties!(ChestType { CHEST_TYPE => "type", });
properties!(ComparatorMode { MODE_COMPARATOR => "mode", });
properties!(DoorHingeSide { DOOR_HINGE => "hinge", });
properties!(NoteBlockInstrument { NOTEBLOCK_INSTRUMENT => "instrument", });
properties!(PistonType { PISTON_TYPE => "type", });
properties!(SlabType { SLAB_TYPE => "type", });
properties!(StairsShape { STAIRS_SHAPE => "shape", });
properties!(StructureMode { STRUCTUREBLOCK_MODE => "mode", });
properties!(BambooLeaves { BAMBOO_LEAVES => "leaves", });
properties!(Tilt { TILT => "tilt", });
properties!(SpeleothemThickness { SPELEOTHEM_THICKNESS => "thickness", });
properties!(SculkSensorPhase { SCULK_SENSOR_PHASE => "sculk_sensor_phase", });
properties!(TrialSpawnerState { TRIAL_SPAWNER_STATE => "trial_spawner_state", });
properties!(VaultState { VAULT_STATE => "vault_state", });
properties!(CreakingHeartState { CREAKING_HEART_STATE => "creaking_heart_state", });
properties!(TestBlockMode { TEST_BLOCK_MODE => "mode", });
properties!(CopperGolemPose { COPPER_GOLEM_POSE => "copper_golem_pose", });
properties!(PotentSulfurState { POTENT_SULFUR_STATE => "potent_sulfur_state", });

/// `SupportType`：一个面能支撑什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportType {
    /// 整个面都是实心的。
    Full,
    /// 面的中心是实心的，比如栅栏的顶面，能放火把。
    Center,
    /// 面的四周是实心的，比如漏斗的顶面，能放铁轨。
    Rigid,
}

impl SupportType {
    const fn first_bit(self) -> u32 {
        match self {
            Self::Full => facts::STURDY_FULL,
            Self::Center => facts::STURDY_CENTER,
            Self::Rigid => facts::STURDY_RIGID,
        }
    }
}

/// `FluidState`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidState(&'static FluidFacts);

impl FluidState {
    /// `isEmpty`。
    pub fn is_empty(self) -> bool {
        self.0.fluid == registries::fluid::EMPTY
    }

    /// `isSource`。
    pub fn is_source(self) -> bool {
        self.0.source
    }

    /// `getAmount`。
    pub fn amount(self) -> i32 {
        i32::from(self.0.amount)
    }

    /// 流体的 `falling` 属性。
    pub fn is_falling(self) -> bool {
        self.0.falling
    }

    /// 流体的注册表 ID，见 [`registries::fluid`]。
    pub fn fluid(self) -> i32 {
        self.0.fluid
    }

    /// `is(Fluids.X)`。官方的 `WATER` 只是水源，流动的水是 `FLOWING_WATER`。
    pub fn is(self, fluid: i32) -> bool {
        self.0.fluid == fluid
    }

    /// `is(FluidTags.X)`。
    pub fn is_tagged(self, tag: FluidTag) -> bool {
        FLUID_TAGS[tag.index].contains(self.0.fluid as usize)
    }

    /// `is(FluidTags.WATER)`。
    pub fn is_water(self) -> bool {
        self.is_tagged(tags::fluid_tags::WATER)
    }

    /// `is(FluidTags.LAVA)`。
    pub fn is_lava(self) -> bool {
        self.is_tagged(tags::fluid_tags::LAVA)
    }

    /// `createLegacyBlock`：这个流体对应的方块，破坏含水方块后留下的就是它。
    pub fn legacy_block(self) -> BlockState {
        BlockState(self.0.legacy_block)
    }

    /// `getOwnHeight`。
    pub fn own_height(self) -> f32 {
        self.amount() as f32 / 9.0
    }
}

/// `item` 在这个物品标签里。
pub fn item_is_tagged(item: i32, tag: ItemTag) -> bool {
    usize::try_from(item).is_ok_and(|id| ITEM_TAGS[tag.index].contains(id))
}

/// `Block.byItem`：物品放置的方块的默认状态，不是方块物品时是空气。
pub fn block_of_item(item: i32) -> BlockState {
    usize::try_from(item)
        .ok()
        .and_then(|id| items::ITEMS.get(id))
        .and_then(|item| item.block)
        .map_or(BlockState::AIR, |id| {
            BlockState(blocks::BLOCKS[id as usize].default_state)
        })
}

/// 一个标签的成员。查询只做下标访问。
struct Bitmap(Box<[u64]>);

impl Bitmap {
    fn of(ids: &[i32]) -> Self {
        let highest = ids.iter().copied().max().unwrap_or(0) as usize;
        let mut words = vec![0_u64; highest / 64 + 1].into_boxed_slice();
        for &id in ids {
            words[id as usize / 64] |= 1 << (id as usize % 64);
        }
        Self(words)
    }

    fn contains(&self, id: usize) -> bool {
        self.0
            .get(id / 64)
            .is_some_and(|word| word & (1 << (id % 64)) != 0)
    }
}

fn bitmaps_of_group(group: usize) -> Vec<Bitmap> {
    tags::TAG_GROUPS[group]
        .tags
        .iter()
        .map(|tag| Bitmap::of(tag.ids))
        .collect()
}

static BLOCK_TAGS: LazyLock<Vec<Bitmap>> =
    LazyLock::new(|| bitmaps_of_group(tags::block_tags::GROUP));
static ITEM_TAGS: LazyLock<Vec<Bitmap>> =
    LazyLock::new(|| bitmaps_of_group(tags::item_tags::GROUP));
static FLUID_TAGS: LazyLock<Vec<Bitmap>> =
    LazyLock::new(|| bitmaps_of_group(tags::fluid_tags::GROUP));

#[cfg(test)]
mod tests {
    use mc_bamboo_data::tags::{block_tags, fluid_tags, item_tags};

    use super::*;

    fn all_states() -> impl Iterator<Item = BlockState> {
        (0..blocks::TOTAL_STATES).map(BlockState)
    }

    #[test]
    fn reads_and_writes_properties() {
        let slab = BlockState(blocks::OAK_SLAB);
        assert_eq!(slab.get(SLAB_TYPE), SlabType::Bottom);
        assert!(!slab.get(WATERLOGGED));

        let wet_top = slab.with(SLAB_TYPE, SlabType::Top).with(WATERLOGGED, true);
        assert_eq!(wet_top.get(SLAB_TYPE), SlabType::Top);
        assert!(wet_top.get(WATERLOGGED));
        assert!(wet_top.is(blocks::OAK_SLAB));
        assert_eq!(
            wet_top
                .with(SLAB_TYPE, SlabType::Bottom)
                .with(WATERLOGGED, false),
            slab
        );
    }

    #[test]
    fn every_value_of_every_property_round_trips() {
        // 每个状态把每个属性改成每个值，再改回来，都要回到原状态。
        for block in blocks::BLOCKS {
            let state = BlockState(block.default_state);
            for def in block.properties {
                let slot = PropertySlot::find(state, def.name).expect("属性来自这个方块");
                for index in 0..def.values.len() {
                    let changed = slot.with_index(state, index);
                    assert_eq!(slot.current_index(changed), index);
                    assert!(changed.is_same_block(state), "{}", block.name);
                }
            }
        }
    }

    #[test]
    fn missing_properties_are_reported() {
        let stone = BlockState(blocks::STONE);
        assert_eq!(stone.try_get(AXIS), None);
        assert!(!stone.has(AXIS));
        assert_eq!(stone.try_with(AXIS, Axis::X), None);

        // 同名不同类型的属性：门的 half 不是楼梯的 half。
        let door = BlockState(blocks::OAK_DOOR);
        assert_eq!(door.try_get(HALF), None);
        assert_eq!(door.get(DOUBLE_BLOCK_HALF), DoubleBlockHalf::Lower);

        // 取值范围不同的同名属性共用一个常量没有关系，值不存在时才失败。
        let hopper = BlockState(blocks::HOPPER);
        assert_eq!(hopper.try_with(FACING_HOPPER, Direction::Up), None);
        assert!(hopper.try_with(FACING_HOPPER, Direction::Down).is_some());
    }

    #[test]
    #[should_panic(expected = "has no property")]
    fn getting_a_missing_property_panics_like_the_game() {
        BlockState(blocks::STONE).get(AXIS);
    }

    #[test]
    fn cycle_wraps_around_in_the_official_order() {
        let door = BlockState(blocks::OAK_DOOR);
        assert!(!door.get(OPEN));
        // 布尔属性的官方顺序是 true、false。
        assert!(door.cycle(OPEN).get(OPEN));
        assert!(!door.cycle(OPEN).cycle(OPEN).get(OPEN));

        let repeater = BlockState(blocks::REPEATER).with(DELAY, 4);
        assert_eq!(repeater.cycle(DELAY).get(DELAY), 1);
    }

    #[test]
    fn every_generated_property_has_a_typed_constant() {
        // 枚举要覆盖全部官方值，否则 `get` 会在运行时 panic。
        fn all_parse<T: PropertyValue>(def: &PropertyDef) -> bool {
            def.values.iter().all(|value| T::from_name(value).is_some())
        }
        for block in blocks::BLOCKS {
            for def in block.properties {
                let known = all_parse::<bool>(def)
                    || all_parse::<i32>(def)
                    || all_parse::<Direction>(def)
                    || all_parse::<Axis>(def)
                    || all_parse::<AttachFace>(def)
                    || all_parse::<BellAttachType>(def)
                    || all_parse::<WallSide>(def)
                    || all_parse::<RedstoneSide>(def)
                    || all_parse::<DoubleBlockHalf>(def)
                    || all_parse::<Half>(def)
                    || all_parse::<SideChainPart>(def)
                    || all_parse::<RailShape>(def)
                    || all_parse::<BedPart>(def)
                    || all_parse::<ChestType>(def)
                    || all_parse::<ComparatorMode>(def)
                    || all_parse::<DoorHingeSide>(def)
                    || all_parse::<NoteBlockInstrument>(def)
                    || all_parse::<PistonType>(def)
                    || all_parse::<SlabType>(def)
                    || all_parse::<StairsShape>(def)
                    || all_parse::<StructureMode>(def)
                    || all_parse::<TestBlockMode>(def)
                    || all_parse::<BambooLeaves>(def)
                    || all_parse::<Tilt>(def)
                    || all_parse::<SpeleothemThickness>(def)
                    || all_parse::<SculkSensorPhase>(def)
                    || all_parse::<TrialSpawnerState>(def)
                    || all_parse::<VaultState>(def)
                    || all_parse::<CreakingHeartState>(def)
                    || all_parse::<CopperGolemPose>(def)
                    || all_parse::<PotentSulfurState>(def)
                    || all_parse::<FrontAndTop>(def);
                assert!(known, "{} 的 {} 没有对应的枚举", block.name, def.name);
            }
        }
    }

    #[test]
    fn instrument_names_are_all_known() {
        for block in blocks::BLOCKS {
            assert!(
                NoteBlockInstrument::from_name(block.instrument).is_some(),
                "{}",
                block.name
            );
        }
        assert_eq!(
            BlockState(blocks::OAK_PLANKS).instrument(),
            NoteBlockInstrument::Bass
        );
    }

    #[test]
    fn tags_and_ancestors_follow_the_official_tables() {
        assert!(BlockState(blocks::AIR).is_tagged(block_tags::REPLACEABLE));
        assert!(!BlockState(blocks::STONE).is_tagged(block_tags::REPLACEABLE));
        assert!(BlockState(blocks::ACTIVATOR_RAIL).is_tagged(block_tags::RAILS));

        assert!(BlockState(blocks::RAIL).is_a(BlockAncestor::BaseRailBlock));
        assert!(!BlockState(blocks::STONE).is_a(BlockAncestor::BaseRailBlock));

        assert!(item_is_tagged(items::CANDLE, item_tags::CANDLES));
        assert!(!item_is_tagged(items::STONE, item_tags::CANDLES));
    }

    #[test]
    fn facts_are_read_per_state() {
        let top_slab = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, SlabType::Top);
        assert!(top_slab.is_face_sturdy(Direction::Up, SupportType::Full));
        assert!(!top_slab.is_face_sturdy(Direction::Down, SupportType::Full));
        let bottom_slab = top_slab.with(SLAB_TYPE, SlabType::Bottom);
        assert!(bottom_slab.is_face_sturdy(Direction::Down, SupportType::Full));
        assert!(!bottom_slab.is_face_sturdy(Direction::Up, SupportType::Full));

        // 栅栏的顶面只有中间能放东西。
        let fence = BlockState(blocks::OAK_FENCE);
        assert!(fence.is_face_sturdy(Direction::Up, SupportType::Center));
        assert!(!fence.is_face_sturdy(Direction::Up, SupportType::Full));
        // 漏斗的顶面只有一圈。
        let hopper = BlockState(blocks::HOPPER);
        assert!(hopper.is_face_sturdy(Direction::Up, SupportType::Rigid));
        assert!(!hopper.is_face_sturdy(Direction::Up, SupportType::Center));
    }

    #[test]
    fn fluid_states_know_water_and_lava() {
        let water = BlockState::WATER.fluid();
        assert!(water.is_water());
        assert!(!water.is_lava());
        assert!(water.is_source());
        assert!(water.is(registries::fluid::WATER));
        assert_eq!(water.amount(), 8);
        assert_eq!(water.legacy_block(), BlockState::WATER);

        let flowing = BlockState::WATER.with(LEVEL, 3).fluid();
        assert!(flowing.is_water());
        assert!(!flowing.is(registries::fluid::WATER));
        assert!(flowing.is(registries::fluid::FLOWING_WATER));
        assert!(flowing.is_tagged(fluid_tags::WATER));

        assert!(BlockState(blocks::STONE).fluid().is_empty());
        assert_eq!(
            BlockState(blocks::STONE).fluid().legacy_block(),
            BlockState::AIR
        );
    }

    #[test]
    fn items_know_their_block() {
        assert_eq!(block_of_item(items::STONE), BlockState(blocks::STONE));
        assert_eq!(block_of_item(items::DIAMOND), BlockState::AIR);
    }

    #[test]
    fn copied_properties_keep_only_what_the_target_has() {
        let source = BlockState(blocks::OAK_STAIRS)
            .with(HORIZONTAL_FACING, Direction::East)
            .with(WATERLOGGED, true);
        let copied = source.copied_onto(BlockState(blocks::SPRUCE_STAIRS).block());
        assert_eq!(copied.get(HORIZONTAL_FACING), Direction::East);
        assert!(copied.get(WATERLOGGED));

        let plain = source.copied_onto(BlockState(blocks::STONE).block());
        assert_eq!(plain, BlockState(blocks::STONE));
    }

    #[test]
    fn every_state_belongs_to_a_block() {
        for state in all_states() {
            assert!(state.block().first_state <= state.0);
        }
    }
}
