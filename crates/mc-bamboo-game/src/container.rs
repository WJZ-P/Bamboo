//! 有物品栏的方块实体：箱子、木桶、末影箱、潜影盒、漏斗、发射器、投掷器。
//!
//! 内容按方块位置存在 [`Region`] 里，只有世界线程碰它。这里还管官方的开合计数
//! （`ContainerOpenersCounter`）和方块事件的排队；事件本身、潜影盒的盖子和动画都在
//! `mc-bamboo-blocks` 的方块事件队列和方块实体里。菜单和点击在 [`crate::menu`]。

use std::collections::BTreeSet;

use mc_bamboo_blocks::{
    BlockState, ShulkerBoxEntity, World, open_lid_box,
    state::{CHEST_TYPE, ChestType, FACING, OPEN},
};
use mc_bamboo_core::{Aabb, BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, registries::menu};
use mc_bamboo_protocol::{item::ItemStack, packet::play::GameMode};

use crate::{
    inbound::PlayerId,
    interaction::{self, BLOCK_INTERACTION_RANGE},
    player::Player,
    region::{Region, RegionTick},
};

/// 官方的 `ContainerOpenersCounter.CHECK_TICK_DELAY`。
const RECHECK_DELAY: u64 = 5;
/// 官方 `stillValidBlockEntity` 和开合计数重查多给的格数。
pub(crate) const CONTAINER_BUFFER: f64 = 4.0;

/// 有物品栏的方块。种类决定槽位数、菜单类型和标题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Chest,
    Barrel,
    EnderChest,
    ShulkerBox,
    Hopper,
    Dispenser,
    Dropper,
}

impl ContainerKind {
    /// 方块实体里能放多少格。末影箱的格子在玩家身上。
    pub(crate) const fn slots(self) -> usize {
        match self {
            Self::Hopper => 5,
            Self::Dispenser | Self::Dropper => 9,
            Self::EnderChest => 0,
            Self::Chest | Self::Barrel | Self::ShulkerBox => 27,
        }
    }

    /// 菜单注册表 ID。
    pub(crate) const fn menu_type(self) -> i32 {
        match self {
            Self::Chest | Self::Barrel | Self::EnderChest => menu::GENERIC_9X3,
            Self::ShulkerBox => menu::SHULKER_BOX,
            Self::Hopper => menu::HOPPER,
            Self::Dispenser | Self::Dropper => menu::GENERIC_3X3,
        }
    }

    /// 没有自定义名字时的标题，官方 `getDefaultName`。
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Chest => "container.chest",
            Self::Barrel => "container.barrel",
            Self::EnderChest => "container.enderchest",
            Self::ShulkerBox => "container.shulkerBox",
            Self::Hopper => "container.hopper",
            Self::Dispenser => "container.dispenser",
            Self::Dropper => "container.dropper",
        }
    }
}

/// 一个容器方块的内容和开合状态。没有内容的容器不留条目。
///
/// 自定义名和锁不在这里，它们是方块实体从物品上带来的组件，见 [`crate::block_items`]。
pub(crate) struct ContainerEntry {
    pub(crate) kind: ContainerKind,
    pub(crate) slots: Vec<ItemStack>,
    /// 官方 `ContainerOpenersCounter.openCount`。潜影盒也用它算盖子开合。
    pub(crate) open_count: i32,
    pub(crate) max_range: f64,
    /// 下一次重查开合人数的 tick，官方 `scheduleRecheck` 的计划刻。
    pub(crate) recheck_at: Option<RegionTick>,
    /// 方块已经没了，等打开的菜单关掉后丢弃。
    pub(crate) dead: bool,
}

impl ContainerEntry {
    fn new(kind: ContainerKind) -> Self {
        let slots = kind.slots();
        Self {
            kind,
            slots: vec![ItemStack::empty(); slots],
            open_count: 0,
            max_range: 0.0,
            recheck_at: None,
            dead: false,
        }
    }
}

/// 每名玩家自己的末影箱，官方 `PlayerEnderChestContainer`。
#[derive(Debug)]
pub(crate) struct EnderChest {
    pub(crate) slots: Vec<ItemStack>,
    /// 官方 `activeChest`：这名玩家正在看的末影箱方块。没有菜单时是 `None`。
    pub(crate) active: Option<BlockPos>,
}

impl Default for EnderChest {
    fn default() -> Self {
        Self {
            slots: vec![ItemStack::empty(); 27],
            active: None,
        }
    }
}

/// 方块属于哪种容器，不是容器时返回 `None`。
///
/// 顺序有讲究：投掷器也是发射器的子类，末影箱不是箱子。
pub(crate) fn kind_of(state: BlockState) -> Option<ContainerKind> {
    [
        (BlockAncestor::EnderChestBlock, ContainerKind::EnderChest),
        (BlockAncestor::ShulkerBoxBlock, ContainerKind::ShulkerBox),
        (BlockAncestor::BarrelBlock, ContainerKind::Barrel),
        (BlockAncestor::HopperBlock, ContainerKind::Hopper),
        (BlockAncestor::DropperBlock, ContainerKind::Dropper),
        (BlockAncestor::DispenserBlock, ContainerKind::Dispenser),
        (BlockAncestor::ChestBlock, ContainerKind::Chest),
    ]
    .into_iter()
    .find(|(ancestor, _)| state.is_a(*ancestor))
    .map(|(_, kind)| kind)
}

/// 这个位置现在是什么容器。
pub(crate) fn kind_at(region: &Region, pos: BlockPos) -> Option<ContainerKind> {
    region
        .chunks
        .block(pos)
        .and_then(|state| kind_of(BlockState(state)))
}

/// 取这个位置的条目，没有就建一个空的。方块换成另一种容器时重建。
pub(crate) fn entry_at(
    region: &mut Region,
    pos: BlockPos,
    kind: ContainerKind,
) -> &mut ContainerEntry {
    region
        .containers
        .entry(pos)
        .and_modify(|entry| {
            if entry.kind != kind {
                *entry = ContainerEntry::new(kind);
            }
        })
        .or_insert_with(|| ContainerEntry::new(kind))
}

/// 潜影盒上面有红石导体或者坐着猫时开不了盖子。这里只看红石导体。
pub(crate) fn blocked(region: &Region, pos: BlockPos) -> bool {
    let above = pos.above();
    region
        .chunks
        .block(above)
        .is_some_and(|state| BlockState(state).is_redstone_conductor())
}

/// 双箱里另一半的方向，官方 `ChestBlock.getConnectedDirection`。
pub(crate) fn connected_direction(state: BlockState) -> Direction {
    let facing = state.get(FACING);
    if state.get(CHEST_TYPE) == ChestType::Left {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    }
}

/// 一个双箱的两半和标题。`first` 是主箱，官方 `DoubleBlockCombiner` 的 FIRST（`RIGHT` 那一半）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoubleChest {
    pub(crate) first: BlockPos,
    pub(crate) second: BlockPos,
}

/// 这个箱子能不能开，能开的话告诉调用方是单箱还是双箱。官方 `ChestBlock.combine`。
///
/// 返回 `Ok(None)` 表示是单箱，`Ok(Some(..))` 是双箱，`Err(())` 表示开不了。
/// 方块在这一刻已经确认是箱子。
pub(crate) fn chest_shape(region: &Region, pos: BlockPos) -> Result<Option<DoubleChest>, ()> {
    // 方块实体不存在或者上方被压住时官方返回 NONE。
    if blocked(region, pos) {
        return Err(());
    }
    let Some(state) = region.chunks.block(pos).map(BlockState) else {
        return Ok(None);
    };
    let chest_type = state.get(CHEST_TYPE);
    if chest_type == ChestType::Single {
        return Ok(None);
    }

    let first_is_here = chest_type == ChestType::Right;
    let neighbour = pos.relative(connected_direction(state));
    if !neighbour_combines(region, neighbour, state) {
        return Ok(None);
    }
    if blocked(region, neighbour) {
        return Err(());
    }
    let (first, second) = if first_is_here {
        (pos, neighbour)
    } else {
        (neighbour, pos)
    };
    Ok(Some(DoubleChest { first, second }))
}

/// `DoubleBlockCombiner` 对邻居的全部要求：同一种方块、不是单箱、类型不同、朝向相同。
fn neighbour_combines(region: &Region, pos: BlockPos, state: BlockState) -> bool {
    let Some(other) = region.chunks.block(pos).map(BlockState) else {
        return false;
    };
    if !other.is_same_block(state) {
        return false;
    }
    let (Some(other_type), Some(other_facing)) = (other.try_get(CHEST_TYPE), other.try_get(FACING))
    else {
        return false;
    };
    other_type != ChestType::Single
        && other_type != state.get(CHEST_TYPE)
        && other_facing == state.get(FACING)
}

/// 菜单构造时调用的 `Container.startOpen`。观察者不计数，由调用方筛掉。
pub(crate) fn start_open(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    match kind {
        ContainerKind::ShulkerBox => start_open_shulker(region, pos),
        ContainerKind::Chest | ContainerKind::Barrel | ContainerKind::EnderChest => {
            increment_openers(region, pos, kind)
        }
        // 漏斗、发射器和投掷器用 `Container` 的默认实现，什么也不做。
        ContainerKind::Hopper | ContainerKind::Dispenser | ContainerKind::Dropper => {}
    }
}

/// 菜单消失时调用的 `Container.stopOpen`。
pub(crate) fn stop_open(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    match kind {
        ContainerKind::ShulkerBox => stop_open_shulker(region, pos),
        ContainerKind::Chest | ContainerKind::Barrel | ContainerKind::EnderChest => {
            decrement_openers(region, pos, kind)
        }
        ContainerKind::Hopper | ContainerKind::Dispenser | ContainerKind::Dropper => {}
    }
}

fn increment_openers(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    let previous = {
        let entry = entry_at(region, pos, kind);
        let previous = entry.open_count;
        entry.open_count += 1;
        previous
    };
    if previous == 0 {
        on_open(region, pos, kind);
        if let Some(entry) = region.containers.get_mut(&pos) {
            let at = region.tick.plus(RECHECK_DELAY);
            entry.recheck_at.get_or_insert(at);
        }
    }
    opener_count_changed(region, pos, kind, previous);
    let range = region
        .containers
        .get(&pos)
        .map_or(0.0, |entry| entry.max_range);
    if let Some(entry) = region.containers.get_mut(&pos) {
        entry.max_range = range.max(BLOCK_INTERACTION_RANGE);
    }
}

fn decrement_openers(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    let previous = {
        let entry = entry_at(region, pos, kind);
        let previous = entry.open_count;
        entry.open_count -= 1;
        previous
    };
    let current = region
        .containers
        .get(&pos)
        .map_or(0, |entry| entry.open_count);
    if current == 0 {
        on_close(region, pos, kind);
        if let Some(entry) = region.containers.get_mut(&pos) {
            entry.max_range = 0.0;
        }
    }
    opener_count_changed(region, pos, kind, previous);
}

/// 从 0 变成有人开。箱子和木桶在官方会播音效，木桶还会改方块状态。
fn on_open(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    if kind == ContainerKind::Barrel {
        set_open(region, pos, true);
    }
}

fn on_close(region: &mut Region, pos: BlockPos, kind: ContainerKind) {
    if kind == ContainerKind::Barrel {
        set_open(region, pos, false);
    }
}

/// 木桶的 `OPEN` 状态，官方 `BarrelBlockEntity.updateBlockState` 的 `setBlockAndUpdate`。
///
/// 走完整的写方块路径：邻居和形状都更新，侦测器、比较器能看到；改动也由 `RegionLevel` 记下，
/// 广播阶段发出去。
fn set_open(region: &mut Region, pos: BlockPos, open: bool) {
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let state = world.block_state(pos);
        if !state.has(OPEN) || state.get(OPEN) == open {
            return;
        }
        world.set_block_and_update(pos, state.with(OPEN, open));
        world.into_effects()
    };
    interaction::send_effects(region, None, &effects);
}

/// `openerCountChanged`：箱子和末影箱把人数排进方块事件队列，客户端靠它开盖子。
///
/// 官方不管人数有没有变都排，队列里重复的事件由集合去掉。
fn opener_count_changed(region: &mut Region, pos: BlockPos, kind: ContainerKind, _previous: i32) {
    if !matches!(kind, ContainerKind::Chest | ContainerKind::EnderChest) {
        return;
    }
    let current = region
        .containers
        .get(&pos)
        .map_or(0, |entry| entry.open_count);
    queue_block_event(region, pos, 1, current);
}

/// 潜影盒的 `startOpen`：不重查人数，直接加一并立刻排事件。
fn start_open_shulker(region: &mut Region, pos: BlockPos) {
    let count = {
        let entry = entry_at(region, pos, ContainerKind::ShulkerBox);
        entry.open_count = entry.open_count.max(0) + 1;
        entry.open_count
    };
    queue_block_event(region, pos, 1, count);
}

fn stop_open_shulker(region: &mut Region, pos: BlockPos) {
    let count = {
        let entry = entry_at(region, pos, ContainerKind::ShulkerBox);
        entry.open_count -= 1;
        entry.open_count
    };
    queue_block_event(region, pos, 1, count);
}

/// `level.blockEvent(pos, block, 1, count)`：排进这一 tick 的方块事件队列。
///
/// 方块已经没了就不排，官方 `ServerLevel.blockEvent` 也不看。队列由 `run_scheduled_ticks`
/// 在随机刻之后统一处理，和官方 `ServerLevel.tick` 的顺序一样。
fn queue_block_event(region: &mut Region, pos: BlockPos, param_a: i32, param_b: i32) {
    let Some(state) = region.chunks.block(pos).map(BlockState) else {
        return;
    };
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    world.block_event(pos, state.block(), param_a, param_b);
}

/// tick 里容器相关的部分：到期的开合人数重查，再丢掉没人用的条目。
///
/// 方块事件和潜影盒动画已经交给 `mc-bamboo-blocks` 的方块事件队列和方块实体 ticker。
pub(crate) fn tick(region: &mut Region) {
    run_rechecks(region);
    forget_unused(region);
}

/// 到期的开合人数重查，官方 `ChestBlock.tick` 里的 `recheckOpen`。
fn run_rechecks(region: &mut Region) {
    let now = region.tick;
    let due: Vec<BlockPos> = region
        .containers
        .iter()
        .filter(|(_, entry)| entry.recheck_at.is_some_and(|at| now.reached(at)))
        .map(|(pos, _)| *pos)
        .collect();
    for pos in due {
        recheck_openers(region, pos);
    }
}

/// 重新数一遍附近还开着这个容器的玩家。官方 `ContainerOpenersCounter.recheckOpeners`。
fn recheck_openers(region: &mut Region, pos: BlockPos) {
    let Some(kind) = kind_at(region, pos) else {
        return;
    };
    let old_range = match region.containers.get_mut(&pos) {
        Some(entry) => {
            entry.recheck_at = None;
            entry.max_range
        }
        None => return,
    };

    let search = Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x) + 1.0,
        f64::from(pos.y) + 1.0,
        f64::from(pos.z) + 1.0,
    )
    .inflated(old_range + CONTAINER_BUFFER);
    let users: Vec<PlayerId> = region
        .players
        .values()
        .filter(|player| {
            player.game_mode != GameMode::Spectator
                && search.intersects(player.bounding_box())
                && has_container_open(region, player, pos, kind)
        })
        .map(|player| player.id)
        .collect();

    if let Some(entry) = region.containers.get_mut(&pos) {
        entry.max_range = 0.0;
    }
    // 官方取的是每个玩家自己的 `getContainerInteractionRange`，本任务里都是 5.0。
    let range = if users.is_empty() {
        0.0
    } else {
        BLOCK_INTERACTION_RANGE
    };

    let previous = region
        .containers
        .get(&pos)
        .map_or(0, |entry| entry.open_count);
    let current = users.len() as i32;
    if previous != current {
        if current != 0 && previous == 0 {
            on_open(region, pos, kind);
        } else if current == 0 {
            on_close(region, pos, kind);
        }
        if let Some(entry) = region.containers.get_mut(&pos) {
            entry.open_count = current;
        }
    }
    opener_count_changed(region, pos, kind, previous);
    if let Some(entry) = region.containers.get_mut(&pos) {
        entry.max_range = range;
        if current == 0 {
            entry.recheck_at = None;
        } else {
            entry.recheck_at = Some(region.tick.plus(RECHECK_DELAY));
        }
    }
}

/// 玩家是不是正开着这个位置上的容器。
fn has_container_open(
    region: &Region,
    player: &Player,
    pos: BlockPos,
    kind: ContainerKind,
) -> bool {
    if kind == ContainerKind::EnderChest {
        return player.ender_chest.active == Some(pos);
    }
    region
        .menus
        .get(&player.id)
        .is_some_and(|menu| menu.target.contains(pos))
}

/// 丢掉没人用的空条目。被菜单引用着的、还有东西的都留着。盖子动画在方块实体里，不归这里。
fn forget_unused(region: &mut Region) {
    let targeted: BTreeSet<BlockPos> = region
        .menus
        .values()
        .flat_map(|menu| menu.target.positions())
        .collect();
    region.containers.retain(|pos, entry| {
        let idle = entry.open_count <= 0 && entry.slots.iter().all(ItemStack::is_empty);
        targeted.contains(pos) || !(entry.dead || idle)
    });
}

/// 方块被破坏。内容没有掉落物，等打开着的菜单关掉后一起丢弃。
pub(crate) fn on_block_destroyed(region: &mut Region, pos: BlockPos) {
    crate::block_items::forget(region, pos);
    let targeted = region.menus.values().any(|menu| menu.target.contains(pos));
    if !targeted {
        region.containers.remove(&pos);
        return;
    }
    if let Some(entry) = region.containers.get_mut(&pos) {
        entry.dead = true;
    }
}

/// 潜影盒能不能开：盖子要抬进的那半格空间没有碰撞。官方 `ShulkerBoxBlock.canOpen`。
pub(crate) fn can_open_shulker(region: &mut Region, pos: BlockPos) -> bool {
    let (mut level, ticks) = region.block_level();
    let world = World::new(&mut level, ticks);
    // 方块实体不存在时盖子就是关着的，仍要检查上方空间。
    if world
        .block_entity::<ShulkerBoxEntity>(pos)
        .is_some_and(|entity| !entity.is_closed())
    {
        return true;
    }
    let state = world.block_state(pos);
    let lid = open_lid_box(pos, state.get(FACING));
    no_collision(&world, lid)
}

/// `CollisionGetter.noCollision` 里和方块有关的那部分。
fn no_collision(world: &World, box_: Aabb) -> bool {
    let x0 = box_.min_x.floor() as i32;
    let x1 = box_.max_x.floor() as i32;
    let y0 = box_.min_y.floor() as i32;
    let y1 = box_.max_y.floor() as i32;
    let z0 = box_.min_z.floor() as i32;
    let z1 = box_.max_z.floor() as i32;
    let query = [box_];
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let pos = BlockPos::new(x, y, z);
                let state = world.block_state(pos);
                let shapes =
                    state.collision_boxes(world, pos, mc_bamboo_blocks::CollisionContext::Empty);
                if shapes.is_empty() {
                    continue;
                }
                let moved: Vec<Aabb> = shapes.iter().map(|shape| shape.at(pos)).collect();
                if mc_bamboo_blocks::shape::join_is_not_empty(
                    &moved,
                    &query,
                    mc_bamboo_blocks::shape::BooleanOp::And,
                ) {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
