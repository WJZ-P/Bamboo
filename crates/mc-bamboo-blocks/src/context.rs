//! 一次右键的上下文，对应官方的 `UseOnContext`、`BlockPlaceContext`、`BlockHitResult`。

use std::ops::Deref;
use std::sync::LazyLock;

use mc_bamboo_core::{Axis, BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use crate::{state::BlockState, world::World};

/// 发起动作的玩家。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Actor {
    /// `player.getUUID()`。告示牌用它认出登记的编辑者。
    pub uuid: [u8; 16],
    pub position: Vec3,
    /// `getYRot`：身体的朝向。`getDirection`、`getRotation` 看它。
    pub yaw: f32,
    /// `getViewYRot(1.0F)`：头的朝向。服务端每 tick 才把它同步成 `yaw`，`orderedByNearest` 看它。
    pub head_yaw: f32,
    pub pitch: f32,
    /// `isSecondaryUseActive`，也就是蹲着。
    pub secondary_use: bool,
    /// 主手物品的注册表 ID，空手是 [`items::AIR`]。
    pub main_hand: i32,
    /// 副手物品的注册表 ID，空手是 [`items::AIR`]。
    pub off_hand: i32,
    /// `getEyeHeight`，站着和蹲着不同。
    pub eye_height: f64,
    /// `blockInteractionRange`。
    pub block_interaction_range: f64,
    /// `player.mayBuild()`：生存模式和创造模式是真，冒险模式和旁观模式是假。
    pub may_build: bool,
    /// `player.preventsBlockDrops()`：创造模式的 `instabuild` 能力。活塞头在玩家破坏时
    /// 只有它为真才拆底座，生存模式改由形状更新拆。
    pub prevents_block_drops: bool,
}

impl Actor {
    /// 这只手上的物品。
    pub const fn item_in(&self, hand: Hand) -> i32 {
        match hand {
            Hand::Main => self.main_hand,
            Hand::Off => self.off_hand,
        }
    }

    /// `getDirection`：身体朝向最接近的水平面。
    pub fn direction(&self) -> Direction {
        Direction::from_yaw(f64::from(self.yaw))
    }

    /// `getEyePosition`。
    pub fn eye_position(&self) -> Vec3 {
        Vec3::new(
            self.position.x,
            self.position.y + self.eye_height,
            self.position.z,
        )
    }

    /// `Item.getPlayerPOVHitResult` 的视线：从眼睛出发，沿着朝向伸出交互距离那么长。
    pub fn view_segment(&self) -> (Vec3, Vec3) {
        let from = self.eye_position();
        let view = Vec3::view_vector(self.yaw, self.pitch);
        (from, from.add(view.scale(self.block_interaction_range)))
    }
}

/// `BlockHitResult`：玩家点中了什么。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHit {
    pub pos: BlockPos,
    pub face: Direction,
    /// 点击点的世界坐标。解码 `use_item_on` 时用 `f64::from(pos) + f64::from(cursor)` 算，
    /// 和官方的精度一样。
    pub location: Vec3,
    pub inside: bool,
}

/// `UseOnContext`。
#[derive(Debug, Clone, Copy)]
pub struct UseContext<'a> {
    pub actor: &'a Actor,
    pub hand: Hand,
    pub hit: BlockHit,
    /// 这只手上的物品，方块的 `useItemOn` 里的 `itemStack`。
    pub item: i32,
    /// 这只手上的物品堆。没有传进来时，组件按这个物品的默认值判断。
    ///
    /// The stack in this hand. When it is absent, component checks use the item's defaults.
    pub stack: Option<&'a ItemStack>,
}

impl<'a> UseContext<'a> {
    pub const fn new(actor: &'a Actor, hand: Hand, hit: BlockHit) -> Self {
        Self {
            actor,
            hand,
            hit,
            item: actor.item_in(hand),
            stack: None,
        }
    }

    /// `getClickedPos`。放置时要用 [`PlaceContext::clicked_pos`]，它可能是相邻的一格。
    pub const fn clicked_pos(&self) -> BlockPos {
        self.hit.pos
    }

    /// `getClickedFace`。
    pub const fn clicked_face(&self) -> Direction {
        self.hit.face
    }

    /// `getClickLocation`。
    pub const fn click_location(&self) -> Vec3 {
        self.hit.location
    }

    /// `isInside`。
    pub const fn is_inside(&self) -> bool {
        self.hit.inside
    }

    /// `getItemInHand().isEmpty()`。
    pub const fn hand_is_empty(&self) -> bool {
        self.item == items::AIR
    }

    /// `getHorizontalDirection`。
    pub fn horizontal_direction(&self) -> Direction {
        self.actor.direction()
    }

    /// `isSecondaryUseActive`。
    pub const fn is_secondary_use_active(&self) -> bool {
        self.actor.secondary_use
    }

    /// `getRotation`。
    pub const fn rotation(&self) -> f32 {
        self.actor.yaw
    }
}

/// `BlockPlaceContext`：点中的方块能被替换时放在原地，否则放在相邻的一格。
#[derive(Debug, Clone, Copy)]
pub struct PlaceContext<'a> {
    context: UseContext<'a>,
    relative_pos: BlockPos,
    replace_clicked: bool,
}

/// `DirectionalPlaceContext` 里没有玩家。放置判定读不到它，这里给一份空的。
/// 没有玩家时 `preventsBlockDrops` 是假。
///
/// `DirectionalPlaceContext` has no player. Placement checks never read this stand-in.
/// `preventsBlockDrops` is false when there is no player.
static NOBODY: LazyLock<Actor> = LazyLock::new(|| Actor {
    uuid: [0; 16],
    position: Vec3::new(0.0, 0.0, 0.0),
    yaw: 0.0,
    head_yaw: 0.0,
    pitch: 0.0,
    secondary_use: false,
    main_hand: items::AIR,
    off_hand: items::AIR,
    eye_height: 0.0,
    block_interaction_range: 0.0,
    may_build: false,
    prevents_block_drops: false,
});

impl<'a> PlaceContext<'a> {
    pub fn new(context: UseContext<'a>, world: &World) -> Self {
        let mut place = Self {
            context,
            relative_pos: context.hit.pos.relative(context.hit.face),
            // 官方的字段初值是 true，`canBeReplaced` 在构造函数里读到的就是这个值。
            replace_clicked: true,
        };
        let clicked = world.block_state(context.hit.pos);
        place.replace_clicked = clicked.can_be_replaced(&place, world);
        place
    }

    /// `BlockPlaceContext.at`：脚手架往上或者往外长时，改成点在 `pos` 的这个面上。
    pub fn at(&self, pos: BlockPos, direction: Direction, world: &World) -> Self {
        let (dx, dy, dz) = direction.normal();
        let location = Vec3::new(
            f64::from(pos.x) + 0.5 + f64::from(dx) * 0.5,
            f64::from(pos.y) + 0.5 + f64::from(dy) * 0.5,
            f64::from(pos.z) + 0.5 + f64::from(dz) * 0.5,
        );
        let hit = BlockHit {
            pos,
            face: direction,
            location,
            inside: false,
        };
        Self::new(
            UseContext {
                hit,
                ..self.context
            },
            world,
        )
    }

    /// `getClickedPos`：方块要放的位置。
    pub const fn clicked_pos(&self) -> BlockPos {
        if self.replace_clicked {
            self.context.hit.pos
        } else {
            self.relative_pos
        }
    }

    /// `canPlace`。
    pub fn can_place(&self, world: &World) -> bool {
        self.replace_clicked
            || world
                .block_state(self.clicked_pos())
                .can_be_replaced(self, world)
    }

    /// `replacingClickedOnBlock`。
    pub const fn replacing_clicked_on_block(&self) -> bool {
        self.replace_clicked
    }

    /// `getNearestLookingDirection`。
    pub fn nearest_looking_direction(&self) -> Direction {
        self.ordered_by_nearest()[0]
    }

    /// `getNearestLookingVerticalDirection`。
    pub fn nearest_looking_vertical_direction(&self) -> Direction {
        let actor = self.context.actor;
        Direction::facing_axis(actor.head_yaw, actor.pitch, Axis::Y)
    }

    /// `getNearestLookingDirections`：放在相邻一格时，点中的那个面排在最前面。
    pub fn nearest_looking_directions(&self) -> [Direction; 6] {
        let mut directions = self.ordered_by_nearest();
        if self.replace_clicked {
            return directions;
        }
        let toward_clicked = self.context.hit.face.opposite();
        if let Some(index) = directions.iter().position(|&side| side == toward_clicked) {
            directions[..=index].rotate_right(1);
        }
        directions
    }

    fn ordered_by_nearest(&self) -> [Direction; 6] {
        let actor = self.context.actor;
        Direction::ordered_by_nearest(actor.head_yaw, actor.pitch)
    }

    /// 这个位置现在的方块。
    pub fn replaced_state(&self, world: &World) -> BlockState {
        world.block_state(self.clicked_pos())
    }
}

impl PlaceContext<'static> {
    /// `DirectionalPlaceContext`：没有玩家，空手，点的是 `pos` 这一格的 `face` 面。
    ///
    /// 下落方块落地时用它判断这一格能不能被替换掉。
    pub fn directional(world: &World, pos: BlockPos, face: Direction) -> Self {
        let location = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        );
        let context = UseContext {
            actor: &NOBODY,
            hand: Hand::Main,
            hit: BlockHit {
                pos,
                face,
                location,
                inside: false,
            },
            item: items::AIR,
            stack: None,
        };
        let mut place = Self {
            context,
            relative_pos: pos,
            replace_clicked: true,
        };
        place.replace_clicked = world.block_state(pos).can_be_replaced(&place, world);
        place
    }
}

impl<'a> Deref for PlaceContext<'a> {
    type Target = UseContext<'a>;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}
