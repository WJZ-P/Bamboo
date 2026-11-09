//! 参数的解析结果。命令执行时从这里取值。

use mc_bamboo_blocks::BlockState;
use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_data::Item;
use uuid::Uuid;

use crate::identifier::Identifier;

/// 官方 `GameType`。枚举值就是官方 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

impl GameMode {
    /// 官方 `GameType.values()` 的顺序。
    pub const ALL: [Self; 4] = [
        Self::Survival,
        Self::Creative,
        Self::Adventure,
        Self::Spectator,
    ];

    /// 官方名，也是命令里写的名字。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
            Self::Adventure => "adventure",
            Self::Spectator => "spectator",
        }
    }

    pub const fn id(self) -> i32 {
        self as i32
    }

    /// 官方 `GameType.byName`。只认小写的官方名。
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// 官方 `GameType.byId`，越界时回到生存模式。
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Creative,
            2 => Self::Adventure,
            3 => Self::Spectator,
            _ => Self::Survival,
        }
    }
}

/// 官方 `MinMaxBounds`，命令里写成 `5`、`5..`、`..5` 或 `5..10`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberBounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

impl<T: PartialOrd + Copy> NumberBounds<T> {
    pub fn contains(&self, value: T) -> bool {
        self.min.is_none_or(|min| min <= value) && self.max.is_none_or(|max| value <= max)
    }
}

/// 官方实体选择器的排序方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Arbitrary,
    Nearest,
    Furthest,
    Random,
}

/// 解析后的实体选择器。Bamboo 目前只有玩家，解析逻辑仍然按官方的选择器结构保存。
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySelector {
    pub max_results: i32,
    pub includes_entities: bool,
    pub world_limited: bool,
    pub distance: Option<NumberBounds<f64>>,
    pub level: Option<NumberBounds<i32>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub delta_x: Option<f64>,
    pub delta_y: Option<f64>,
    pub delta_z: Option<f64>,
    pub rotation_x: Option<NumberBounds<f32>>,
    pub rotation_y: Option<NumberBounds<f32>>,
    pub order: SortOrder,
    pub current_entity: bool,
    pub player_name: Option<String>,
    pub uuid: Option<Uuid>,
    /// 选择器类型通过 `limitToType` 记住的实体类型，只影响官方内部的候选范围。
    pub type_name: Option<String>,
    /// 名字过滤。第二个字段是官方的 `!` 取反。
    pub name_filters: Vec<(String, bool)>,
    pub type_filters: Vec<(String, bool)>,
    pub type_tag_filters: Vec<(String, bool)>,
    pub game_mode_filters: Vec<(GameMode, bool)>,
    pub team_filters: Vec<(String, bool)>,
    pub tag_filters: Vec<(String, bool)>,
    /// `scores={目标=范围}`。同一个目标写两次时，后面的覆盖前面的。
    ///
    /// `scores={objective=range}`. A later entry for the same objective replaces the earlier one.
    pub score_filters: Vec<(String, NumberBounds<i32>)>,
    pub uses_selectors: bool,
}

impl EntitySelector {
    /// 官方的 `isSelfSelector`。
    pub const fn is_self_selector(&self) -> bool {
        self.current_entity
    }

    /// 选择器里写了 `x`/`y`/`z` 时，官方把基准点换成这些坐标。
    pub fn position(&self, origin: Vec3) -> Vec3 {
        Vec3::new(
            self.x.unwrap_or(origin.x),
            self.y.unwrap_or(origin.y),
            self.z.unwrap_or(origin.z),
        )
    }

    /// 官方 `EntitySelector.getResultLimit`：只有 `sort=arbitrary` 时按 `limit` 截断，
    /// 其余排序要先把候选全部排完才能取前几个。
    pub const fn result_limit(&self) -> i32 {
        if matches!(self.order, SortOrder::Arbitrary) {
            self.max_results
        } else {
            i32::MAX
        }
    }
}

/// 官方 `EntityAnchorArgument.Anchor`：从脚底还是眼睛出发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityAnchor {
    Feet,
    Eyes,
}

impl EntityAnchor {
    pub const ALL: [Self; 2] = [Self::Feet, Self::Eyes];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Feet => "feet",
            Self::Eyes => "eyes",
        }
    }

    /// 官方 `Anchor.getByName`。只认小写。
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|anchor| anchor.name() == name)
    }
}

/// 官方 `WorldCoordinate`：`~` 表示相对，其余是绝对坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldCoordinate {
    pub relative: bool,
    pub value: f64,
}

impl WorldCoordinate {
    pub const fn absolute(value: f64) -> Self {
        Self {
            relative: false,
            value,
        }
    }

    /// 按基准值算出最终坐标。
    pub fn get(self, origin: f64) -> f64 {
        if self.relative {
            origin + self.value
        } else {
            self.value
        }
    }
}

/// 官方 `Coordinates`。`^` 开头的是以执行者朝向为基准的局部坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coordinates {
    World {
        x: WorldCoordinate,
        y: WorldCoordinate,
        z: WorldCoordinate,
    },
    Local {
        left: f64,
        up: f64,
        forwards: f64,
    },
}

impl Coordinates {
    pub const fn absolute(x: f64, y: f64, z: f64) -> Self {
        Self::World {
            x: WorldCoordinate::absolute(x),
            y: WorldCoordinate::absolute(y),
            z: WorldCoordinate::absolute(z),
        }
    }

    /// 官方 `Coordinates.getPosition`。`source` 是执行者的脚底位置。
    pub fn position(self, source: Vec3, yaw: f32, pitch: f32) -> Vec3 {
        match self {
            Self::World { x, y, z } => Vec3::new(x.get(source.x), y.get(source.y), z.get(source.z)),
            Self::Local { left, up, forwards } => {
                apply_local(source, yaw, pitch, left, up, forwards)
            }
        }
    }

    pub fn is_x_relative(self) -> bool {
        match self {
            Self::World { x, .. } => x.relative,
            Self::Local { .. } => true,
        }
    }

    pub fn is_y_relative(self) -> bool {
        match self {
            Self::World { y, .. } => y.relative,
            Self::Local { .. } => true,
        }
    }

    pub fn is_z_relative(self) -> bool {
        match self {
            Self::World { z, .. } => z.relative,
            Self::Local { .. } => true,
        }
    }
}

/// `LocalCoordinates.apply`。一次性把三个轴算出来，调用方只关心最终位置。
fn apply_local(source: Vec3, yaw: f32, pitch: f32, left: f64, up: f64, forwards: f64) -> Vec3 {
    // 官方的乘法在 float 上做完，再传给 `Mth.cos`（参数是 double）。
    let degrees = (std::f64::consts::PI / 180.0) as f32;
    let y_cos = mth::cos(f64::from((yaw + 90.0) * degrees));
    let y_sin = mth::sin(f64::from((yaw + 90.0) * degrees));
    let x_cos = mth::cos(f64::from(-pitch * degrees));
    let x_sin = mth::sin(f64::from(-pitch * degrees));
    let x_cos_up = mth::cos(f64::from((-pitch + 90.0) * degrees));
    let x_sin_up = mth::sin(f64::from((-pitch + 90.0) * degrees));

    let forward = Vec3::new(
        f64::from(y_cos * x_cos),
        f64::from(x_sin),
        f64::from(y_sin * x_cos),
    );
    let upward = Vec3::new(
        f64::from(y_cos * x_cos_up),
        f64::from(x_sin_up),
        f64::from(y_sin * x_cos_up),
    );
    let leftward = cross(forward, upward).scale(-1.0);

    Vec3::new(
        source.x + forward.x * forwards + upward.x * up + leftward.x * left,
        source.y + forward.y * forwards + upward.y * up + leftward.y * left,
        source.z + forward.z * forwards + upward.z * up + leftward.z * left,
    )
}

fn cross(first: Vec3, second: Vec3) -> Vec3 {
    Vec3::new(
        first.y * second.z - first.z * second.y,
        first.z * second.x - first.x * second.z,
        first.x * second.y - first.y * second.x,
    )
}

/// 官方 `RotationArgument` 的结果：第一个数是水平角，第二个数是俯仰角。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub yaw: WorldCoordinate,
    pub pitch: WorldCoordinate,
}

impl Rotation {
    /// 官方 `Coordinates.getRotation`，返回 `(水平角, 俯仰角)`。
    pub fn resolve(self, source_yaw: f32, source_pitch: f32) -> (f32, f32) {
        (
            self.yaw.get(f64::from(source_yaw)) as f32,
            self.pitch.get(f64::from(source_pitch)) as f32,
        )
    }
}

/// 官方 `blocks.BlockStateParser.parseForBlock` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStateInput {
    pub state: BlockState,
    /// 命令里明确写出来的属性，顺序就是解析顺序。谓词只看这些属性。
    pub properties: Vec<(String, String)>,
}

/// 官方 `BlockPredicateArgument` 的两种结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockPredicateInput {
    /// 指定方块和写出来的属性。
    State(BlockStateInput),
    /// 方块标签和模糊属性。
    Tag {
        tag: String,
        block_ids: Vec<i32>,
        properties: Vec<(String, String)>,
    },
}

impl BlockPredicateInput {
    /// 一个状态是否满足谓词。官方 `BlockPredicate.test`，不带 NBT。
    pub fn test(&self, state: BlockState) -> bool {
        match self {
            Self::State(input) => {
                if !state.is_same_block(input.state) {
                    return false;
                }
                input
                    .properties
                    .iter()
                    .all(|(name, value)| state.property_text(name) == Some(value.as_str()))
            }
            Self::Tag {
                block_ids,
                properties,
                ..
            } => {
                if !block_ids.contains(&(state.block_id() as i32)) {
                    return false;
                }
                properties
                    .iter()
                    .all(|(name, value)| state.property_text(name) == Some(value.as_str()))
            }
        }
    }
}

/// 官方 `ItemParser` 的结果。Bamboo 不支持物品组件，所以只有物品本身。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemInput {
    pub item: &'static Item,
}

/// 官方 `ItemPredicateArgument` 的结果，Bamboo 只支持物品和物品标签。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemPredicateInput {
    Item(&'static Item),
    Tag { tag: String, item_ids: Vec<i32> },
}

impl ItemPredicateInput {
    pub fn test(&self, item: &'static Item) -> bool {
        match self {
            // `ITEMS` 是 `const` 切片，每次使用地址都可能不同，不能比指针。
            //
            // `ITEMS` is a `const` slice, so its address can differ on each use. Compare names.
            Self::Item(expected) => expected.name == item.name,
            Self::Tag { item_ids, .. } => {
                let id = mc_bamboo_data::items::ITEMS
                    .iter()
                    .position(|candidate| candidate.name == item.name);
                id.is_some_and(|id| item_ids.contains(&(id as i32)))
            }
        }
    }
}

/// 官方 `MessageArgument.Message`。`text` 是整段原文，`parts` 是选择器所在的位置。
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub text: String,
    pub parts: Vec<MessagePart>,
}

/// 计分项准则。`hearts` 为真时默认渲染成心。
///
/// An objective criterion. `hearts` means the default render type is hearts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveCriteria {
    pub name: String,
    pub read_only: bool,
    pub hearts: bool,
}

/// 官方 `OperationArgument` 的九种运算。
///
/// The nine operations of official `OperationArgument`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreOperation {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Min,
    Max,
    Swap,
}

/// 计分板显示位置，id 跟官方 `DisplaySlot` 一致。
///
/// A scoreboard display slot. The id matches official `DisplaySlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySlot(pub u8);

impl DisplaySlot {
    const NAMES: [&'static str; 19] = [
        "list",
        "sidebar",
        "below_name",
        "sidebar.team.black",
        "sidebar.team.dark_blue",
        "sidebar.team.dark_green",
        "sidebar.team.dark_aqua",
        "sidebar.team.dark_red",
        "sidebar.team.dark_purple",
        "sidebar.team.gold",
        "sidebar.team.gray",
        "sidebar.team.dark_gray",
        "sidebar.team.blue",
        "sidebar.team.green",
        "sidebar.team.aqua",
        "sidebar.team.red",
        "sidebar.team.light_purple",
        "sidebar.team.yellow",
        "sidebar.team.white",
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .position(|slot| *slot == name)
            .map(|id| Self(id as u8))
    }

    pub fn name(self) -> &'static str {
        Self::NAMES.get(self.0 as usize).copied().unwrap_or("list")
    }

    pub fn suggestions() -> &'static [&'static str] {
        &Self::NAMES
    }
}

/// 队伍颜色，id 跟官方 `TeamColor` 一致。
///
/// A team color. The id matches official `TeamColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamColorId(pub u8);

impl TeamColorId {
    const NAMES: [&'static str; 16] = [
        "black",
        "dark_blue",
        "dark_green",
        "dark_aqua",
        "dark_red",
        "dark_purple",
        "gold",
        "gray",
        "dark_gray",
        "blue",
        "green",
        "aqua",
        "red",
        "light_purple",
        "yellow",
        "white",
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .position(|color| *color == name)
            .map(|id| Self(id as u8))
    }

    pub fn name(self) -> &'static str {
        Self::NAMES.get(self.0 as usize).copied().unwrap_or("white")
    }

    pub fn suggestions() -> &'static [&'static str] {
        &Self::NAMES
    }
}

/// 记分板名字。`*`、一段文字或一个选择器，执行时再决定是玩家还是纯名字。
///
/// A score holder. `*`, a piece of text, or a selector. Execution decides whether
/// it is a player or a name-only holder.
#[derive(Debug, Clone, PartialEq)]
pub enum ScoreHolderSpec {
    Wildcard,
    Text(String),
    Selector(EntitySelector),
}

/// 消息里的一段实体选择器。位置是字符下标，从消息开头算。
#[derive(Debug, Clone, PartialEq)]
pub struct MessagePart {
    pub start: usize,
    pub end: usize,
    pub selector: EntitySelector,
}

/// 所有参数类型的解析结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Entity(Box<EntitySelector>),
    BlockPos(Coordinates),
    Vec3(Coordinates),
    Rotation(Rotation),
    BlockState(BlockStateInput),
    BlockPredicate(BlockPredicateInput),
    Item(ItemInput),
    ItemPredicate(ItemPredicateInput),
    Message(Message),
    GameMode(GameMode),
    EntityAnchor(EntityAnchor),
    Time(i32),
    Criteria(ObjectiveCriteria),
    Operation(ScoreOperation),
    DisplaySlot(DisplaySlot),
    ScoreHolder(ScoreHolderSpec),
    /// 已经按官方文本编解码重写过的组件标签。
    ///
    /// A component tag already rewritten by the official text codec.
    Component(mc_bamboo_nbt::Tag),
    /// 已经校验过的样式标签。
    ///
    /// A style tag that has already been checked.
    Style(mc_bamboo_nbt::Tag),
    TeamColor(TeamColorId),
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match self {
            Self::Long(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_double(&self) -> Option<f64> {
        match self {
            Self::Double(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_entity(&self) -> Option<&EntitySelector> {
        match self {
            Self::Entity(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn as_block_pos(&self) -> Option<Coordinates> {
        match self {
            Self::BlockPos(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<Coordinates> {
        match self {
            Self::Vec3(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_rotation(&self) -> Option<Rotation> {
        match self {
            Self::Rotation(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_block_state(&self) -> Option<&BlockStateInput> {
        match self {
            Self::BlockState(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_block_predicate(&self) -> Option<&BlockPredicateInput> {
        match self {
            Self::BlockPredicate(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_item(&self) -> Option<ItemInput> {
        match self {
            Self::Item(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_item_predicate(&self) -> Option<&ItemPredicateInput> {
        match self {
            Self::ItemPredicate(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_message(&self) -> Option<&Message> {
        match self {
            Self::Message(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_game_mode(&self) -> Option<GameMode> {
        match self {
            Self::GameMode(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_entity_anchor(&self) -> Option<EntityAnchor> {
        match self {
            Self::EntityAnchor(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_time(&self) -> Option<i32> {
        match self {
            Self::Time(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_criteria(&self) -> Option<&ObjectiveCriteria> {
        match self {
            Self::Criteria(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_operation(&self) -> Option<ScoreOperation> {
        match self {
            Self::Operation(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_display_slot(&self) -> Option<DisplaySlot> {
        match self {
            Self::DisplaySlot(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_score_holder(&self) -> Option<&ScoreHolderSpec> {
        match self {
            Self::ScoreHolder(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_component_tag(&self) -> Option<&mc_bamboo_nbt::Tag> {
        match self {
            Self::Component(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_style_tag(&self) -> Option<&mc_bamboo_nbt::Tag> {
        match self {
            Self::Style(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_team_color(&self) -> Option<TeamColorId> {
        match self {
            Self::TeamColor(value) => Some(*value),
            _ => None,
        }
    }
}

/// 官方 `Mth.wrapDegrees(float)`。
pub fn wrap_degrees(angle: f32) -> f32 {
    let mut normalized = angle % 360.0;
    if normalized >= 180.0 {
        normalized -= 360.0;
    }
    if normalized < -180.0 {
        normalized += 360.0;
    }
    normalized
}

/// 从注册名里去掉 `minecraft:`，官方显示物品、方块名时用这个形式。
pub fn short_name(identifier: &Identifier) -> String {
    if identifier.namespace() == "minecraft" {
        identifier.path().to_owned()
    } else {
        identifier.to_registry_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_modes_follow_the_official_names() {
        assert_eq!(GameMode::by_name("creative"), Some(GameMode::Creative));
        assert_eq!(GameMode::by_name("Creative"), None);
        assert_eq!(GameMode::by_id(3), GameMode::Spectator);
        assert_eq!(GameMode::by_id(9), GameMode::Survival);
        assert_eq!(GameMode::Adventure.id(), 2);
    }

    #[test]
    fn bounds_accept_open_ends() {
        let range = NumberBounds {
            min: Some(2.0),
            max: None,
        };
        assert!(range.contains(2.0));
        assert!(!range.contains(1.5));
        let exact = NumberBounds {
            min: Some(4),
            max: Some(4),
        };
        assert!(exact.contains(4));
        assert!(!exact.contains(5));
    }

    #[test]
    fn world_coordinates_are_relative_when_asked() {
        assert_eq!(WorldCoordinate::absolute(3.0).get(10.0), 3.0);
        let relative = WorldCoordinate {
            relative: true,
            value: 0.0,
        };
        assert_eq!(relative.get(10.0), 10.0);
    }

    #[test]
    fn entity_anchors_follow_the_official_names() {
        assert_eq!(EntityAnchor::by_name("feet"), Some(EntityAnchor::Feet));
        assert_eq!(EntityAnchor::by_name("eyes"), Some(EntityAnchor::Eyes));
        assert_eq!(EntityAnchor::by_name("Eyes"), None);
    }

    #[test]
    fn wrap_degrees_matches_mth() {
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
        assert_eq!(wrap_degrees(45.0), 45.0);
    }
}
