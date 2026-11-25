//! `TransportItemsBetweenContainers`：铜傀儡在铜箱子（源）与普通/陷阱箱子（目标）
//! 之间搬物品的行为。逐条照官方 26.3 字节码：找目标、行走 / 排队 / 60 刻交互、
//! 位置集合记忆与 140 刻冷却。
//!
//! `TransportItemsBetweenContainers`: the copper golem's item-transport behaviour
//! between copper chests (source) and regular / trapped chests (destination). Follows
//! the 26.3 bytecode: target search, travelling / queuing / the 60-tick interaction,
//! the position-set memories and the 140-tick cooldown.

use mc_bamboo_core::{Aabb, Axis, BlockPos, Direction, Vec3};
use mc_bamboo_data::{blocks, tags::block_tags};
use mc_bamboo_protocol::{
    components::value::GlobalPos, item::ItemStack, packet::play::EquipmentSlot,
};

use super::{
    HORIZONTAL_SEARCH_RADIUS, IDLE_COOLDOWN, MAX_UNREACHABLE_POSITIONS, MAX_VISITED_POSITIONS,
    State, TARGET_INTERACTION_TIME, TRANSPORTED_ITEM_MAX_STACK_SIZE, VERTICAL_SEARCH_RADIUS,
    VISITED_POSITIONS_MEMORY_TIME,
};
use crate::{
    container,
    entity::{
        ctx::{BlockView, Command},
        mob::{
            MobType,
            brain::{
                Cx,
                memory::{MemoryType, MemoryValue, PositionTracker, WalkTarget},
            },
            living,
        },
    },
    shape::{self, CollisionContext},
    state::{CHEST_TYPE, ChestType},
};

/// 官方 `GlobalPos` 的维度键：Bamboo 目前只跑主世界，两处位置集合都用这一个维度。
///
/// The dimension key of vanilla's `GlobalPos`: Bamboo only runs the overworld, so both
/// position sets use this one dimension.
const OVERWORLD: &str = "minecraft:overworld";

/// `TransportItemsBetweenContainers.TransportItemState`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportState {
    #[default]
    Travelling,
    Queuing,
    Interacting,
}

/// `ContainerInteractionState`：60 刻交互进入时按箱子内容敲定的那一个分支。
/// 名字照官方枚举，统一后缀由 clippy 允许。
#[allow(
    clippy::enum_variant_names,
    reason = "照官方 ContainerInteractionState 的四个枚举名"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interaction {
    PickupItem,
    PickupNoItem,
    PlaceItem,
    PlaceNoItem,
}

impl Interaction {
    /// `CopperGolemAi.getTargetReachedInteractions`：进入交互时的动画状态和第 9 刻的音效。
    fn animation(self) -> (State, &'static str) {
        match self {
            Self::PickupItem => (
                State::GettingItem,
                "minecraft:entity.copper_golem.no_item_get",
            ),
            Self::PickupNoItem => (
                State::GettingNoItem,
                "minecraft:entity.copper_golem.no_item_no_get",
            ),
            Self::PlaceItem => (
                State::DroppingItem,
                "minecraft:entity.copper_golem.item_drop",
            ),
            Self::PlaceNoItem => (
                State::DroppingNoItem,
                "minecraft:entity.copper_golem.item_no_drop",
            ),
        }
    }
}

/// `TransportItemTarget`：只存方块位置；内容和方块实体在每次查询时从只读视图现读，
/// 与「精确读当前状态、并行读本刻快照」的容器查询一致。
///
/// Only the block position is stored; the contents and block entity are read afresh
/// from the read-only view at each query, matching the live / snapshot container views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub pos: BlockPos,
}

/// `TransportItemsBetweenContainers` 的实例字段，挂在 `Behavior::Transport` 上。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transport {
    /// `target`，`null` 表示当前没有目标。
    pub target: Option<Target>,
    /// `state`，初值 `TRAVELLING`。
    pub transport_state: TransportState,
    /// `interactionState`，进入 60 刻交互时敲定。
    pub interaction: Option<Interaction>,
    /// `ticksSinceReachingTarget`。
    pub interaction_tick: i32,
}

impl Transport {
    /// `checkExtraStartConditions` / `canStillUse`：拴绳未建模，恒为不拴。
    pub fn can_use(&self, cx: &Cx) -> bool {
        !cx.memories.has(MemoryType::TransportItemsCooldownTicks)
            && !cx.memories.has(MemoryType::IsPanicking)
            && cx.mob.mob_type == MobType::CopperGolem
    }

    /// `TransportItemsBetweenContainers.tick`。
    pub fn tick(&mut self, cx: &mut Cx) {
        if std::env::var_os("CG_DEBUG").is_some() {
            eprintln!(
                "cg tick time={} state={:?} it={} target={:?}",
                cx.time, self.transport_state, self.interaction_tick, self.target
            );
        }
        let invalid = self.update_invalid_target(cx);
        if self.target.is_none() {
            self.stop(cx);
            return;
        }
        if invalid {
            return;
        }
        match self.transport_state {
            TransportState::Queuing => {
                let Some(target) = self.target else {
                    return;
                };
                if !another_mob_interacting(cx, target.pos) {
                    self.transport_state = TransportState::Travelling;
                    walk_towards_target(cx, target.pos);
                }
            }
            TransportState::Travelling => self.on_travel(cx),
            TransportState::Interacting => self.on_reached_target(cx),
        }
    }

    /// `Behavior.stop`：回到行走阶段、清掉正在开的箱子并设回 IDLE。
    pub fn stop(&mut self, cx: &mut Cx) {
        self.on_start_travelling(cx);
        cx.mob.navigation.stop();
        cx.memories.erase(MemoryType::WalkTarget);
    }

    /// `onStartTravelling`：调 `onTravelling` 消费者（铜傀儡清开箱位置 + IDLE），
    /// 状态回 TRAVELLING、交互状态和计数清零。
    fn on_start_travelling(&mut self, cx: &mut Cx) {
        cx.mob.copper_golem.opened_chest_pos = None;
        cx.mob.copper_golem.state = State::Idle;
        self.transport_state = TransportState::Travelling;
        self.interaction = None;
        self.interaction_tick = 0;
    }

    /// `stopTargetingCurrentTarget`：清目标、停导航、擦 WALK_TARGET。
    fn stop_targeting_current_target(&mut self, cx: &mut Cx) {
        self.interaction_tick = 0;
        self.target = None;
        cx.mob.navigation.stop();
        cx.memories.erase(MemoryType::WalkTarget);
    }

    /// `clearMemoriesAfterMatchingTargetFound`。
    fn clear_memories_after_match(&mut self, cx: &mut Cx) {
        self.stop_targeting_current_target(cx);
        cx.memories.erase(MemoryType::VisitedBlockPositions);
        cx.memories
            .erase(MemoryType::UnreachableTransportBlockPositions);
    }

    /// `enterCooldownAfterNoMatchingTargetFound`：140 刻冷却，清两处位置集合。
    fn enter_cooldown(&mut self, cx: &mut Cx) {
        self.stop_targeting_current_target(cx);
        cx.memories.set(
            MemoryType::TransportItemsCooldownTicks,
            MemoryValue::Int(IDLE_COOLDOWN),
        );
        cx.memories.erase(MemoryType::VisitedBlockPositions);
        cx.memories
            .erase(MemoryType::UnreachableTransportBlockPositions);
    }

    /// `updateInvalidTarget`：目标失效就重新找，找不到进冷却。
    fn update_invalid_target(&mut self, cx: &mut Cx) -> bool {
        if self.has_valid_target(cx) {
            return false;
        }
        self.stop_targeting_current_target(cx);
        let Some(target) = find_transport_target(cx) else {
            self.enter_cooldown(cx);
            return true;
        };
        self.target = Some(target);
        self.on_start_travelling(cx);
        self.set_visited_block_pos(cx, target.pos);
        true
    }

    /// `hasValidTarget`：目标非空、方块种类仍合适、箱子没被挡住、路径还能走。
    fn has_valid_target(&mut self, cx: &mut Cx) -> bool {
        let Some(target) = self.target else {
            return false;
        };
        let blocks = cx.ctx.blocks();
        if !is_wanted_block(blocks, cx.mob, target.pos) || is_target_blocked(blocks, target.pos) {
            return false;
        }
        if self.transport_state != TransportState::Travelling {
            return true;
        }
        // `hasValidTravellingPath` 在导航还没有路径时先按官方 `createPath` 算一条，
        // 但不装进导航；这里只在已经有一条路径之后才校验可达性。
        if cx.mob.navigation.path.is_none() {
            return true;
        }
        if self.has_valid_travelling_path(cx, target.pos) {
            return true;
        }
        self.mark_unreachable(cx, target.pos);
        false
    }

    /// `hasValidTravellingPath`：路径终点（或当前位置）与箱子碰撞盒相交，
    /// 或者从该点能看到箱子六个面之一。
    fn has_valid_travelling_path(&self, cx: &mut Cx, pos: BlockPos) -> bool {
        let blocks = cx.ctx.blocks();
        let path_end = cx
            .mob
            .navigation
            .path
            .as_ref()
            .and_then(|path| path.end_node())
            .map(|node| node.pos());
        let from = match path_end {
            Some(end) => Vec3::new(
                f64::from(end.x) + 0.5,
                f64::from(end.y),
                f64::from(end.z) + 0.5,
            ),
            None => cx.me.position,
        };
        let from = middle_y(from, cx.me.bounding_box());
        let range = interaction_range(cx);
        let within = within_target_distance(blocks, cx.me, pos, range, from);
        let no_path = cx.mob.navigation.path.is_none();
        if !no_path && within {
            return true;
        }
        if no_path && !within {
            return false;
        }
        within || can_see_any_target_side(cx, pos, from)
    }

    fn on_travel(&mut self, cx: &mut Cx) {
        let Some(target) = self.target else {
            return;
        };
        let center = middle_y(cx.me.position, cx.me.bounding_box());
        let blocks = cx.ctx.blocks();
        if within_target_distance(blocks, cx.me, target.pos, 3.0, center)
            && another_mob_interacting(cx, target.pos)
        {
            stop_in_place(cx);
            self.transport_state = TransportState::Queuing;
            return;
        }
        let range = interaction_range(cx);
        let in_range = within_target_distance(blocks, cx.me, target.pos, range, center);
        if in_range {
            self.start_on_reached_target_interaction(cx, target.pos);
        } else {
            walk_towards_target(cx, target.pos);
        }
    }

    /// `startOnReachedTargetInteraction`：按箱子内容敲定四种交互分支之一。
    fn start_on_reached_target_interaction(&mut self, cx: &mut Cx, pos: BlockPos) {
        self.interaction = Some(reached_interaction(cx, pos));
        self.transport_state = TransportState::Interacting;
    }

    /// `onReachedTarget`：每刻看箱子、停步、按交互状态触发动画 / 音效；
    /// 第 60 刻真正取放物品（发 Command）并回到行走阶段。
    fn on_reached_target(&mut self, cx: &mut Cx) {
        let Some(target) = self.target else {
            return;
        };
        let center = middle_y(cx.me.position, cx.me.bounding_box());
        if !within_target_distance(cx.ctx.blocks(), cx.me, target.pos, 2.0, center) {
            self.on_start_travelling(cx);
            return;
        }
        cx.memories.set(
            MemoryType::LookTarget,
            MemoryValue::Look(PositionTracker::at_block(target.pos)),
        );
        stop_in_place(cx);
        self.interaction_tick += 1;
        if let Some(interaction) = self.interaction {
            let (state, sound) = interaction.animation();
            match self.interaction_tick {
                1 => {
                    // `ChestBlockEntity.startOpen` → `signalOpenCount` → `playSound`：
                    // 世界随机源取一次 `nextFloat()` 算开盖音高。
                    let _ = cx.ctx.blocks().level_random_float();
                    cx.mob.copper_golem.opened_chest_pos = Some(target.pos);
                    cx.mob.copper_golem.state = state;
                }
                9 => living::play_sound(cx.me, cx.mob, sound, 1.0, 1.0),
                60 => {
                    // `stopOpen` 同样取一次世界随机数算关盖音高。
                    let _ = cx.ctx.blocks().level_random_float();
                    cx.mob.copper_golem.opened_chest_pos = None;
                }
                _ => {}
            }
        }
        if self.interaction_tick >= TARGET_INTERACTION_TIME {
            let action = reached_interaction(cx, target.pos);
            let slots = cx.ctx.blocks().container_slots(target.pos);
            match action {
                Interaction::PickupItem => {
                    // 官方 `pickUpItems` 在行为里直接 setItemSlot；这里改的是
                    // `Cx.mob`（`me.kind` 此刻被借走），箱子内容由命令串行落地。
                    let taken = slots
                        .iter()
                        .find(|stack| !stack.is_empty())
                        .cloned()
                        .unwrap_or_else(ItemStack::empty);
                    if !taken.is_empty() {
                        let mut capped = taken.clone();
                        capped.count = capped.count.min(TRANSPORTED_ITEM_MAX_STACK_SIZE);
                        cx.mob.equipment.set(EquipmentSlot::MainHand, capped);
                        cx.ctx
                            .emit(Command::CopperGolemPickUp { pos: target.pos }, cx.me);
                        self.clear_memories_after_match(cx);
                    } else {
                        self.stop_targeting_current_target(cx);
                    }
                }
                Interaction::PickupNoItem | Interaction::PlaceNoItem => {
                    self.stop_targeting_current_target(cx);
                }
                Interaction::PlaceItem => {
                    let held = cx.mob.equipment.get(EquipmentSlot::MainHand).clone();
                    let remaining = simulate_put_down(&slots, &held);
                    cx.mob.equipment.set(EquipmentSlot::MainHand, remaining);
                    cx.ctx.emit(
                        Command::CopperGolemPutDown {
                            pos: target.pos,
                            held,
                        },
                        cx.me,
                    );
                    self.clear_memories_after_match(cx);
                }
            }
            self.on_start_travelling(cx);
        }
    }

    /// `setVisitedBlockPos`：`Set<GlobalPos>` 追加，超过 10 个进冷却。
    fn set_visited_block_pos(&mut self, cx: &mut Cx, pos: BlockPos) {
        let mut visited = visited_positions(cx);
        visited.push(GlobalPos {
            dimension: OVERWORLD.to_owned(),
            pos,
        });
        if visited.len() > MAX_VISITED_POSITIONS {
            self.enter_cooldown(cx);
        } else {
            cx.memories.set_with_ttl(
                MemoryType::VisitedBlockPositions,
                MemoryValue::GlobalPosList(visited),
                VISITED_POSITIONS_MEMORY_TIME,
            );
        }
    }

    /// `markVisitedBlockPosAsUnreachable`：从 visited 移除、加进 unreachable。
    fn mark_unreachable(&mut self, cx: &mut Cx, pos: BlockPos) {
        let global = GlobalPos {
            dimension: OVERWORLD.to_owned(),
            pos,
        };
        let mut visited = visited_positions(cx);
        visited.retain(|entry| entry != &global);
        let mut unreachable = unreachable_positions(cx);
        if !unreachable.contains(&global) {
            unreachable.push(global);
        }
        if unreachable.len() > MAX_UNREACHABLE_POSITIONS {
            self.enter_cooldown(cx);
        } else {
            cx.memories.set_with_ttl(
                MemoryType::VisitedBlockPositions,
                MemoryValue::GlobalPosList(visited),
                VISITED_POSITIONS_MEMORY_TIME,
            );
            cx.memories.set_with_ttl(
                MemoryType::UnreachableTransportBlockPositions,
                MemoryValue::GlobalPosList(unreachable),
                VISITED_POSITIONS_MEMORY_TIME,
            );
        }
    }
}

/// `Memories.get(VISITED_BLOCK_POSITIONS)`；空集合当不存在。
fn visited_positions(cx: &Cx) -> Vec<GlobalPos> {
    cx.memories
        .global_pos_list(MemoryType::VisitedBlockPositions)
        .unwrap_or(&[])
        .to_vec()
}

fn unreachable_positions(cx: &Cx) -> Vec<GlobalPos> {
    cx.memories
        .global_pos_list(MemoryType::UnreachableTransportBlockPositions)
        .unwrap_or(&[])
        .to_vec()
}

/// `getInteractionRange`：路径走完是 1.0，否则 0.5。
fn interaction_range(cx: &Cx) -> f64 {
    if cx.mob.navigation.path.is_some() && cx.mob.navigation.is_done() {
        1.0
    } else {
        0.5
    }
}

/// `setMiddleYPosition`：位置抬高半个碰撞盒高度。
fn middle_y(position: Vec3, box_: Aabb) -> Vec3 {
    Vec3::new(
        position.x,
        position.y + box_.size(Axis::Y) / 2.0,
        position.z,
    )
}

/// `isWithinTargetDistance`：以实体碰撞盒大小造一个以 `from` 为中心的盒子，
/// 和「箱子碰撞形状的包围盒按 (d, 0.5, d) 外扩」求交。
fn within_target_distance(
    blocks: &dyn BlockView,
    me: &crate::entity::Entity,
    pos: BlockPos,
    distance: f64,
    from: Vec3,
) -> bool {
    let body = me.bounding_box();
    let mob_box = Aabb::new(
        from.x - body.size(Axis::X) / 2.0,
        from.y - body.size(Axis::Y) / 2.0,
        from.z - body.size(Axis::Z) / 2.0,
        from.x + body.size(Axis::X) / 2.0,
        from.y + body.size(Axis::Y) / 2.0,
        from.z + body.size(Axis::Z) / 2.0,
    );
    let state = blocks.block_state(pos);
    let boxes = shape::collision_boxes(state, pos, CollisionContext::Empty);
    let mut target_box = Aabb::new(1.0, 1.0, 1.0, 0.0, 0.0, 0.0);
    for bounds in boxes.iter() {
        target_box = union(target_box, *bounds);
    }
    if target_box.min_x > target_box.max_x {
        return false;
    }
    let inflated = target_box.inflate(distance, 0.5, distance).at(pos);
    mob_box.intersects(inflated)
}

fn union(a: Aabb, b: Aabb) -> Aabb {
    Aabb::new(
        a.min_x.min(b.min_x),
        a.min_y.min(b.min_y),
        a.min_z.min(b.min_z),
        a.max_x.max(b.max_x),
        a.max_y.max(b.max_y),
        a.max_z.max(b.max_z),
    )
}

/// `canSeeAnyTargetSide`：从 `from` 朝箱子中心六个方向偏移 0.5 做射线，
/// 命中目标格就算看得见。这里用只读视图的 `clip_collider`。
fn can_see_any_target_side(cx: &Cx, pos: BlockPos, from: Vec3) -> bool {
    let center = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    );
    let context = CollisionContext::Empty;
    for direction in Direction::ALL {
        let (dx, dy, dz) = direction.normal();
        let to = center.add(Vec3::new(
            f64::from(dx) * 0.5,
            f64::from(dy) * 0.5,
            f64::from(dz) * 0.5,
        ));
        if cx
            .ctx
            .blocks()
            .clip_collider(from, to, context)
            .is_some_and(|hit| hit.pos == pos)
        {
            return true;
        }
    }
    false
}

/// `stopInPlace`：停导航、清零水平输入、只保留竖直速度。
fn stop_in_place(cx: &mut Cx) {
    cx.mob.navigation.stop();
    cx.mob.mover.speed_modifier = 0.0;
    cx.me.delta.x = 0.0;
    cx.me.delta.z = 0.0;
}

/// `BehaviorUtils.setWalkAndLookTargetMemories(mob, pos, 1.0, 0)`。
fn walk_towards_target(cx: &mut Cx, pos: BlockPos) {
    let point = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    );
    cx.memories.set(
        MemoryType::LookTarget,
        MemoryValue::Look(PositionTracker::at_point(point)),
    );
    cx.memories.set(
        MemoryType::WalkTarget,
        MemoryValue::Walk(WalkTarget::to_point(point, 1.0, 0)),
    );
}

/// `doReachedTargetInteraction`：按主手和箱子内容敲定四种交互分支。
fn reached_interaction(cx: &Cx, pos: BlockPos) -> Interaction {
    let held = cx.mob.equipment.get(EquipmentSlot::MainHand);
    let slots = cx.ctx.blocks().container_slots(pos);
    if held.is_empty() {
        if slots.iter().any(|stack| !stack.is_empty()) {
            Interaction::PickupItem
        } else {
            Interaction::PickupNoItem
        }
    } else if slots.is_empty()
        || slots
            .iter()
            .any(|stack| !stack.is_empty() && stack.item == held.item)
    {
        Interaction::PlaceItem
    } else {
        Interaction::PlaceNoItem
    }
}

/// `isAnotherMobInteractingWithTarget` / `shouldQueueForTarget`：
/// Bamboo 没有 `ChestBlockEntity.getEntitiesWithContainerOpen` 表，
/// 用「别的铜傀儡当前开着的箱子位置」判定（同一个大箱子的两半也算）。
fn another_mob_interacting(cx: &Cx, pos: BlockPos) -> bool {
    let mut found = false;
    let area = Aabb::new(
        f64::from(pos.x) - 2.0,
        f64::from(pos.y) - 2.0,
        f64::from(pos.z) - 2.0,
        f64::from(pos.x) + 3.0,
        f64::from(pos.y) + 3.0,
        f64::from(pos.z) + 3.0,
    );
    cx.ctx.for_each_other_in(area, &mut |other| {
        if let crate::entity::KindPublic::Mob(mob) = &other.kind
            && mob.opened_chest_pos == Some(pos)
        {
            found = true;
        }
    });
    found
}

/// `isWantedBlock`：空手找铜箱子（源），拿着物品找普通 / 陷阱箱子（目标）。
fn is_wanted_block(blocks: &dyn BlockView, mob: &crate::entity::mob::Mob, pos: BlockPos) -> bool {
    let state = blocks.block_state(pos);
    if mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
        state.is_tagged(block_tags::COPPER_CHESTS)
    } else {
        state.is(blocks::CHEST) || state.is(blocks::TRAPPED_CHEST)
    }
}

/// `ChestBlock.isChestBlockedAt`：`isBlockedChestByBlock` 这一段（上方一格是
/// 红石导体）。`isCatSittingOnChest` 那一段没有猫的坐下位置表，按官方对拍夹具
/// 的「没有猫」处理，见 README。
fn is_target_blocked(blocks: &dyn BlockView, pos: BlockPos) -> bool {
    blocks.block_state(pos.above()).is_redstone_conductor()
}

/// `getTransportTarget`：搜索盒内全部箱子，按 `distToCenterSqr` 取最近的有效目标。
fn find_transport_target(cx: &Cx) -> Option<Target> {
    let blocks = cx.ctx.blocks();
    let passenger = cx.me.vehicle.is_some();
    let horizontal = if passenger {
        1
    } else {
        HORIZONTAL_SEARCH_RADIUS
    };
    let vertical = if passenger { 1 } else { VERTICAL_SEARCH_RADIUS };
    let origin = cx.me.block_position();
    let candidates = blocks.container_positions_in(origin, horizontal, vertical);
    let visited = visited_positions(cx);
    let unreachable = unreachable_positions(cx);
    let mut best: Option<Target> = None;
    let mut best_distance = f64::MAX;
    for pos in candidates {
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        let distance = cx.me.position.distance_to_sqr(center);
        if distance >= best_distance {
            continue;
        }
        // `AABB(blockPosition).inflate(horizontal, vertical, horizontal)`：
        // 盒子的下界是 `origin - radius`，上界是 `origin + 1 + radius`。
        if f64::from(pos.x) < f64::from(origin.x - horizontal)
            || f64::from(pos.x) >= f64::from(origin.x + horizontal + 1)
            || f64::from(pos.y) < f64::from(origin.y - vertical)
            || f64::from(pos.y) >= f64::from(origin.y + vertical + 1)
            || f64::from(pos.z) < f64::from(origin.z - horizontal)
            || f64::from(pos.z) >= f64::from(origin.z + horizontal + 1)
        {
            continue;
        }
        if !is_wanted_block(blocks, cx.mob, pos) {
            continue;
        }
        let connected = connected_chest_positions(blocks, pos);
        if connected.iter().any(|at| {
            visited
                .iter()
                .chain(unreachable.iter())
                .any(|entry| entry.pos == *at)
        }) {
            continue;
        }
        if is_target_blocked(blocks, pos) {
            continue;
        }
        best = Some(Target { pos });
        best_distance = distance;
    }
    best
}

/// `getConnectedTargets`：大箱子的两半（单箱子只有自己）。
fn connected_chest_positions(blocks: &dyn BlockView, pos: BlockPos) -> Vec<BlockPos> {
    let state = blocks.block_state(pos);
    if !state.has(CHEST_TYPE) || state.get(CHEST_TYPE) == ChestType::Single {
        return vec![pos];
    }
    let direction = crate::behaviour::machines::chest::connected_direction(state);
    let other = pos.relative(direction);
    if blocks.has_block_entity(other) {
        vec![pos, other]
    } else {
        vec![pos]
    }
}

/// `addItemsToContainer` 对一份只读槽位快照的推演，用来在行为里同步主手剩余量；
/// 真正改箱子的是 `Command::CopperGolemPutDown` 的落地阶段。
fn simulate_put_down(slots: &[ItemStack], held: &ItemStack) -> ItemStack {
    if held.is_empty() {
        return ItemStack::empty();
    }
    let mut rest = held.clone();
    for stack in slots {
        if stack.is_empty() {
            return ItemStack::empty();
        }
        if container::same_item_same_components(stack, &rest)
            && stack.count < crate::container::item_max(stack)
        {
            let space = crate::container::item_max(stack) - stack.count;
            rest.count -= space;
            if rest.is_empty() {
                return ItemStack::empty();
            }
        }
    }
    rest
}
