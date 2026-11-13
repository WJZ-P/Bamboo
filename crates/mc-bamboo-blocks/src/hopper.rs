//! 漏斗，官方 `HopperBlockEntity` 和它用的取放物品算法。
//!
//! 每刻把朝向那一格的容器塞一个物品，再从上面一格的容器吸一个，冷却 8 刻。
//! 堆肥桶没有物品栏，但是官方把它也当成容器，见 [`crate::composter`]。
//! 上面不是容器、也没有被完整方块挡住时，吸吸取盒里的物品实体。
//!
//! When the block above is not a container and is not a full cube blocking the hopper,
//! item entities in the suck box are pulled in.

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction, mth};
use mc_bamboo_data::{BlockAncestor, tags::block_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::{BlockEntity, Watch, block_entity_clock},
    container::{
        CONTAINER_STACK_LIMIT, Container, item_max, remove_item, same_item_same_components,
    },
    entity::Entity,
    shape::Aabb,
    state::{ENABLED, FACING_HOPPER},
    world::World,
};

/// `HopperBlockEntity.MOVE_ITEM_SPEED`。
pub const MOVE_ITEM_SPEED: i32 = 8;
/// `HopperBlockEntity.HOPPER_CONTAINER_SIZE`。
pub const HOPPER_CONTAINER_SIZE: usize = 5;
/// `HopperBlockEntity.NO_COOLDOWN_TIME`。
pub const NO_COOLDOWN_TIME: i32 = -1;

/// `HopperBlockEntity`。槽位内容在 [`Container`] 里，这里只有冷却和时间。
pub struct HopperEntity {
    pos: BlockPos,
    /// `cooldownTime`。刚放下时是 -1。
    cooldown_time: i32,
    /// `tickedGameTime` 的相对表示：`now - 上一次真跑的 gameTime`。
    ///
    /// 官方每刻无条件把它写成 `level.getGameTime()`，但只在同一刻里拿两个漏斗
    /// 比大小（决定目标漏斗少不等那一刻），所以绝对基准可以整体平移。
    /// `None` = 官方读盘 / 刚放下的初值 0（绝对时刻），也就是「很久以前」。
    last_tick_age: Option<i64>,
    /// `last_tick_age` 是在哪个 `gameTime` 量出来的；读路径按当前刻补上差值。
    last_tick_now: i64,
    /// 冷却期睡眠。官方这几天里只把 `cooldownTime` 减 1、把时间字段写成世界时间，
    /// 两者都按下面的公式现算，不落到字段里。
    sleep: Option<HopperSleep>,
    /// 这一刻的 `tryMoveItems` 返回 false、并且没有别的可见改动，可以睡到外部叫醒。
    idle: bool,
    /// 已经在闲置睡眠里。读路径按「每刻都跑过」现算 `tickedGameTime`。
    idle_sleeping: bool,
    /// 上次真跑时登记的关注范围，供 `watch()` 交给方块实体表。
    watch: Option<Watch>,
}

/// 冷却期里 `cooldownTime` 每刻 -1、`tickedGameTime` 每刻跟世界时间走。
#[derive(Clone, Copy)]
struct HopperSleep {
    /// 真跑完一次 `pushItemsTick` 的那刻。
    since: i64,
    /// 冷却归零、必须真跑的那一格。
    wake_at: i64,
    /// `since` 那刻的冷却值。
    cooldown: i32,
}

impl HopperEntity {
    /// `newBlockEntity`：刚放下的漏斗。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            cooldown_time: NO_COOLDOWN_TIME,
            last_tick_age: None,
            last_tick_now: 0,
            sleep: None,
            idle: false,
            idle_sleeping: false,
            watch: None,
        }
    }

    /// `isOnCooldown`：还在冷却里。睡着的按公式现算。
    pub fn is_on_cooldown(&self) -> bool {
        self.cooldown_time() > 0
    }

    /// `isOnCustomCooldown`：冷却比正常的一轮长。只在往空漏斗里放东西时用到。
    pub fn is_on_custom_cooldown(&self) -> bool {
        self.cooldown_time() > MOVE_ITEM_SPEED
    }

    /// `setCooldown`。外部调用前 `wake` 已经把数值算到当前刻。
    pub fn set_cooldown(&mut self, cooldown_time: i32) {
        self.cooldown_time = cooldown_time;
        self.sleep = None;
        self.idle = false;
        self.idle_sleeping = false;
    }

    /// 现在的冷却时间，测试和存档用；睡着的按公式现算。
    pub fn cooldown_time(&self) -> i32 {
        self.cooldown_at(block_entity_clock())
    }

    /// `tickedGameTime`：按当前刻还原绝对时刻，和官方每刻写的值一致。
    pub fn ticked_game_time(&self) -> i64 {
        self.ticked_game_time_at(block_entity_clock())
    }

    /// 读路径用当前刻现算 `now - age`；`None` 是官方读盘后的初值 0。
    fn ticked_game_time_at(&self, now: i64) -> i64 {
        match self.age_at(now) {
            Some(age) => now.saturating_sub(age),
            None => 0,
        }
    }

    /// 相对量，供录制层 / 缓存键用；`None` 对应官方读盘后的初值 0。
    pub fn ticked_game_time_age(&self) -> Option<i64> {
        self.age_at(block_entity_clock())
    }

    /// 安静刻里官方每刻都真跑过，所以时间字段一直是「新鲜」的（age 0）。
    /// 冻结 / 装盒时睡眠已经结束，差值按经过的刻数增长，和官方一样停在原值。
    fn age_at(&self, now: i64) -> Option<i64> {
        if self.sleep.is_some() || self.idle_sleeping {
            return Some(0);
        }
        self.last_tick_age
            .map(|age| age.saturating_add(now.saturating_sub(self.last_tick_now)))
    }

    /// 安静刻的冷却：`since` 之后每刻 -1，到点那刻归零。
    fn cooldown_at(&self, now: i64) -> i32 {
        match self.sleep {
            Some(sleep) => {
                let steps = now
                    .saturating_sub(sleep.since)
                    .min(sleep.wake_at.saturating_sub(sleep.since))
                    .max(0);
                sleep
                    .cooldown
                    .saturating_sub(i32::try_from(steps).unwrap_or(i32::MAX))
            }
            None => self.cooldown_time,
        }
    }

    /// 真跑一次 `pushItemsTick` 之前，把睡眠按官方口径推进到上一刻结束。
    fn begin_tick(&mut self, now: i64) {
        self.materialize_at(now.saturating_sub(1));
    }

    /// 真跑完一次 `pushItemsTick`：相对时间归零。
    fn note_tick(&mut self, now: i64) {
        self.last_tick_age = Some(0);
        self.last_tick_now = now;
        self.sleep = None;
        self.idle_sleeping = false;
    }

    /// 结束睡眠 / 冻结恢复：把冷却和相对时间按公式算到 `tick` 那刻结束。
    /// `tick` 用 `now - 1` 时，当前刻的减量留给真跑的 `pushItemsTick`。
    fn materialize_at(&mut self, tick: i64) {
        let now = block_entity_clock();
        // 闲置睡眠的安静刻官方仍每刻写时间字段，装盒 / 冻结 / 叫醒都要按
        // 「刚跑过」的口径还原；冷却期睡眠则按公式推进。
        if self.idle_sleeping {
            self.sleep = None;
            // 官方在安静刻每刻都跑过：到 `tick` 为止最后一次是 `tick`。
            self.last_tick_age = Some(now.saturating_sub(tick).max(0));
            self.last_tick_now = now;
            self.cooldown_time = 0;
            return;
        }
        if let Some(sleep) = self.sleep.take() {
            let steps = tick
                .saturating_sub(sleep.since)
                .min(sleep.wake_at.saturating_sub(sleep.since))
                .max(0);
            self.cooldown_time = sleep
                .cooldown
                .saturating_sub(i32::try_from(steps).unwrap_or(i32::MAX));
            // 官方在安静刻里每刻都跑过：到 `tick` 为止最后一次是 `tick`。
            self.last_tick_age = Some(now.saturating_sub(tick).max(0));
        } else {
            self.last_tick_age = self.age_at(now);
        }
        self.last_tick_now = now;
    }

    /// 真跑完一次 `tryMoveItems`：记下能不能睡到外部叫醒，以及要关注哪些格子 / 盒子。
    fn note_idle(&mut self, idle: bool, pos: BlockPos, facing: Direction) {
        self.idle = idle;
        if !idle {
            self.watch = None;
            return;
        }
        let above = pos.above();
        let destination = pos.relative(facing);
        // 官方读取的全部输入：自己的状态 / 物品、朝向那一格的方块和容器、
        // 上方一格的方块和容器，以及吸取盒里的物品实体。
        self.watch = Some(Watch {
            cells: vec![pos, above, destination],
            boxes: vec![suck_box(pos)],
        });
    }

    /// 存档读入冷却；`tickedGameTime` 官方不存，读回是构造时的 0。
    pub fn set_cooldown_from_archive(&mut self, cooldown: i32) {
        self.cooldown_time = cooldown;
        self.last_tick_age = None;
        self.last_tick_now = 0;
        self.sleep = None;
        self.idle = false;
        self.idle_sleeping = false;
        self.watch = None;
    }
}

impl BlockEntity for HopperEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        push_items_tick(world, self.pos, self);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn sleep_from(&mut self, now: i64) -> Option<i64> {
        // 闲置漏斗：没有物品可搬、上方没有容器能给出物品、吸取盒里没有物品实体。
        // 官方这一刻也只是减冷却、写时间字段，接下来每刻都一样，直到某个
        // 关注格或吸取盒发生变化。装盒时按「醒着」处理（见下面的 i64::MAX）。
        if self.idle {
            self.idle = false;
            self.idle_sleeping = true;
            return Some(i64::MAX);
        }
        self.watch = None;
        // 冷却期里每刻只减 1、写世界时间，没有别的可见效果。到冷却归零
        // 那一刻必须真跑；物品实体 / 邻居变化 / `ENABLED` 由外部写统一叫醒。
        let cooldown = self.cooldown_time;
        if cooldown <= 0 {
            self.sleep = None;
            return None;
        }
        let wake_at = now.saturating_add(i64::from(cooldown));
        self.sleep = Some(HopperSleep {
            since: now,
            wake_at,
            cooldown,
        });
        Some(wake_at)
    }

    fn materialize(&mut self, tick: i64) {
        self.materialize_at(tick);
        self.idle = false;
        self.idle_sleeping = false;
        self.watch = None;
    }

    fn watch(&self) -> Option<Watch> {
        self.watch.clone()
    }

    fn skips_border_pause(&self) -> bool {
        self.idle_sleeping
    }

    fn resleep(&mut self, since: i64, wake_at: i64) {
        // 闲置睡眠跨表没有意义：放回后真跑一次，再按当前状态重新睡。
        if wake_at == i64::MAX || wake_at.saturating_sub(since) == i64::MAX {
            self.cooldown_time = 0;
            self.last_tick_age = Some(0);
            self.last_tick_now = since;
            self.sleep = None;
            self.idle = false;
            self.idle_sleeping = false;
            self.watch = None;
            return;
        }
        // 装盒时冷却已按公式算到源表的最后一刻。放回的 ticker 是醒着的，
        // 这一刻会真跑一次 `pushItemsTick`（自己减这一刻的 1），跑完再由
        // `sleep_from` 重新排日程。这里若重新登记睡眠，读路径会把这一刻的
        // 减量提前算掉，实体阶段的 `entityInside` 就会比官方少看到一刻冷却。
        let cooldown = i32::try_from(wake_at.saturating_sub(since).max(1)).unwrap_or(i32::MAX);
        self.cooldown_time = cooldown;
        self.last_tick_age = Some(0);
        self.last_tick_now = since;
        self.sleep = None;
        self.idle = false;
        self.idle_sleeping = false;
        self.watch = None;
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        // 官方 HopperBlockEntity.saveAdditional 在公共容器键之外只写冷却；
        // tickedGameTime 是绝对时刻，官方不存。
        Ok(vec![(
            "TransferCooldown".to_owned(),
            Tag::Int(self.cooldown_time()),
        )])
    }
}

/// 在 `pos` 的容器上运行一段只读代码。这一格不是容器时是 `None`。
///
/// [`Level::with_container`](crate::Level::with_container) 不返回泛型结果，是为了让
/// `Level` 能当 trait object 用；要结果就借这个函数带出来。
pub(crate) fn query<R>(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&dyn Container) -> R,
) -> Option<R> {
    world.peek_container(pos, |container| f(container))
}

/// `HopperBlockEntity.pushItemsTick`。滴答器每刻调用它。
pub fn push_items_tick(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) {
    let now = block_entity_clock();
    entity.begin_tick(now);
    entity.cooldown_time -= 1;
    // 官方每刻写 `tickedGameTime = level.getGameTime()`，会读一次世界时间。
    // 这里只记相对量：读路径按当前刻还原，录制层不再看到这次时间读。
    entity.note_tick(now);
    if !entity.is_on_cooldown() {
        entity.set_cooldown(0);
        let writes = world.content_writes();
        let changed = try_move_items(world, pos, entity);
        let state = world.block_state(pos);
        let facing = state.get(FACING_HOPPER);
        // 只有「没事可做、也没有部分吸入 / 送出」的漏斗才睡到外部叫醒。
        // 冷却期、失败的搬运、被红石锁住都按官方的逐刻推进留在醒着的集合。
        let idle = !changed && world.content_writes() == writes && !world.hopper_has_items(pos);
        entity.note_idle(idle, pos, facing);
    }
}

/// `tryMoveItems`：先往外送，送不出去再看能不能往里吸。
fn try_move_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    if entity.is_on_cooldown() {
        return false;
    }
    let state = world.block_state(pos);
    if !state.get(ENABLED) {
        return false;
    }

    let mut changed = false;
    if !container_is_empty(world, pos) {
        changed = eject_items(world, pos, entity);
    }
    if !inventory_full(world, pos) {
        changed |= suck_in_items(world, pos, entity);
    }
    if changed {
        entity.set_cooldown(MOVE_ITEM_SPEED);
        set_changed(world, pos);
    }
    changed
}

/// 漏斗自己的物品栏是不是空的。
fn container_is_empty(world: &mut World, pos: BlockPos) -> bool {
    query(world, pos, &mut |container| container.is_empty()).unwrap_or(true)
}

/// `inventoryFull`：每一格都有东西，而且数量都到物品自己的堆叠上限。
fn inventory_full(world: &mut World, pos: BlockPos) -> bool {
    query(world, pos, &mut |container| {
        (0..container.size()).all(|slot| {
            let stack = container.get(slot);
            !stack.is_empty() && stack.count == item_max(&stack)
        })
    })
    .unwrap_or(false)
}

/// `ejectItems`：把朝向那一格的容器塞满一个物品。
fn eject_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    let facing = world.block_state(pos).get(FACING_HOPPER);
    let destination = pos.relative(facing);
    let direction = facing.opposite();

    // `getContainerAt`：目标不是容器时什么也不做。世界里的实体不是容器。
    let full = query(world, destination, &mut |container| {
        is_full_container(container, Some(direction))
    });
    if full != Some(false) {
        return false;
    }

    for slot in 0..HOPPER_CONTAINER_SIZE {
        let Some(item) = query(world, pos, &mut |container| container.get(slot)) else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        // 官方先 `removeItem(1)` 再 `addItem`，失败时靠同一个对象把整堆放回去。
        // 这里两侧都是静默写：内容真变了才叫醒关注这两格的睡眠漏斗。
        let mut taken = ItemStack::empty();
        let mut inserted = None;
        let source = world.silent_container(pos, &mut |container| {
            taken = remove_item(container, slot, 1);
        });
        if source.is_none() {
            continue;
        }
        let destination_written = world.silent_container(destination, &mut |container| {
            inserted = Some(add_item(container, taken.clone(), Some(direction)));
        });
        let Some(inserted) = inserted.filter(|_| destination_written.is_some()) else {
            world.silent_container(pos, &mut |container| container.set(slot, item.clone()));
            continue;
        };

        if inserted.remaining.is_empty() {
            // `addItem` 成功时两侧的静默写已经各自通知过。冷却和比较器通知照官方。
            let destination_state = world.block_state(destination);
            if inserted.was_empty && destination_state.is_a(BlockAncestor::HopperBlock) {
                apply_hopper_cooldown(world, destination, Some(entity.ticked_game_time()));
            }
            set_changed(world, destination);
            return true;
        }

        // 放不进去，槽位恢复成原来的整堆。
        world.silent_container(pos, &mut |container| container.set(slot, item.clone()));
    }
    false
}

/// 往空漏斗里放东西时，官方让目标漏斗少等一刻。
///
/// `from_ticked_game_time` 是来源的位置上也有漏斗时的 `tickedGameTime`。来源不是漏斗
/// （比如投掷器）时是 `None`，目标漏斗不会少等。
pub(crate) fn apply_hopper_cooldown(
    world: &mut World,
    pos: BlockPos,
    from_ticked_game_time: Option<i64>,
) {
    world.with_hopper(pos, &mut |destination| {
        if destination.is_on_custom_cooldown() {
            return;
        }
        let skip = i32::from(
            from_ticked_game_time.is_some_and(|from| destination.ticked_game_time() >= from),
        );
        destination.set_cooldown(MOVE_ITEM_SPEED - skip);
    });
}

/// 物品实体的盒子和漏斗方块重叠时，`HopperBlock.entityInside` 会试着吸它。
///
/// 吸取盒比方块高。只落在漏斗上方、没有碰到这一格的物品，要等漏斗自己的 tick
/// 走 `suckInItems`，不在这里吸。
///
/// When the item overlaps the hopper block, `HopperBlock.entityInside` tries to pull it in.
///
/// The suck box is taller than the block. An item that only sits above the hopper, without
/// touching this cell, waits for the hopper tick's `suckInItems`.
/// `ItemEntity.tick` 发 `SuckIntoHopper` 之前扫的格子。精确和并行两种驱动、
/// 闲置漏斗能不能睡，都必须用这一份判定：差一格就会漏一次 `entityInside`。
///
/// The cells an item can trigger `entityInside` on. Both entity drivers and the
/// idle-hopper sleep gate use this exact range.
pub fn hopper_cells_touched(bounds: Aabb) -> (BlockPos, BlockPos) {
    (
        BlockPos::new(
            mth::floor(bounds.min_x),
            mth::floor(bounds.min_y) - 2,
            mth::floor(bounds.min_z),
        ),
        BlockPos::new(
            mth::floor(bounds.max_x),
            mth::floor(bounds.max_y),
            mth::floor(bounds.max_z),
        ),
    )
}

/// 这一件物品会不会对 `pos` 这一格触发 `HopperBlock.entityInside`。
/// 判定和 [`intersects_block`] / [`intersects_suck_box`] 共用同一组形状。
///
/// Whether this item triggers `entityInside` for the hopper at `pos`.
pub fn item_touches_hopper(item: &Entity, pos: BlockPos) -> bool {
    intersects_block(item, pos) && intersects_suck_box(item, pos)
}

/// `ItemEntity.tick` 发 `SuckIntoHopper` 之前用 `any_hopper_in` 扫的那片格子里，
/// 真正有漏斗方块的位置。精确 / 并行两种驱动和闲置漏斗睡眠门控共用它。
///
/// The hopper cells in the range that `ItemEntity.tick` scans before emitting
/// `SuckIntoHopper`. Shared by both entity drivers and the idle sleep gate.
pub fn entity_inside_cells(world: &World, bounds: Aabb) -> Vec<BlockPos> {
    let (min, max) = hopper_cells_touched(bounds);
    let mut found = Vec::new();
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let pos = BlockPos::new(x, y, z);
                if world
                    .block_state(pos)
                    .is_a(mc_bamboo_data::BlockAncestor::HopperBlock)
                {
                    found.push(pos);
                }
            }
        }
    }
    found
}

pub(crate) fn suck_touching_item(world: &mut World, item: &mut Entity) {
    if item.removed || entity_inside_cells(world, item.bounding_box()).is_empty() {
        return;
    }
    let Some(ground) = item.ground() else {
        return;
    };
    if ground.stack.is_empty() {
        return;
    }
    let hoppers = entity_inside_cells(world, item.bounding_box());
    for pos in hoppers {
        if item.removed {
            break;
        }
        // 官方先要求实体碰到这一格（`Shapes.block()`），再要求吸取盒。
        // Vanilla requires the entity to touch this cell (`Shapes.block()`), then the suck box.
        if !item_touches_hopper(item, pos) {
            continue;
        }
        world.with_block_entity_for_entity::<HopperEntity, _>(pos, |hopper, world| {
            try_move_touching(world, pos, hopper, item);
        });
    }
}

/// `suckInItems`：从上面一格的容器里拿一个物品。
///
/// 上方没有容器时改去吸落在漏斗里和上方的物品实体，见 [`suck_touching_item`]。
///
/// When the block above is not a container, items sitting in and above the hopper are pulled in
/// instead. See [`suck_touching_item`].
fn suck_in_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    let above = pos.above();
    // 只读探测：上方不是容器时改去吸物品实体。官方的 `getSourceContainer` 只读，
    // 不该把上面那个漏斗叫醒。
    let Some(slots) = world.peek_container(above, |container| {
        container.slots_for_face(Direction::Down).map_or_else(
            || (0..container.size()).collect::<Vec<_>>(),
            <[usize]>::to_vec,
        )
    }) else {
        return suck_ground_items(world, pos, entity);
    };
    // 上面的容器也是漏斗时，`from instanceof HopperBlockEntity` 成立。
    let mut from_ticked_game_time = None;
    if world.block_state(above).is_a(BlockAncestor::HopperBlock) {
        world.peek_hopper(above, &mut |hopper| {
            from_ticked_game_time = Some(hopper.ticked_game_time());
        });
    }
    // `canTakeItem(into, slot, stack)` 看的是目标漏斗此刻的整张槽位表，不是即将取出的那一个。
    // 成功拿走一件就返回，所以这一份快照够用。
    //
    // `canTakeItem(into, slot, stack)` looks at the destination hopper's whole slot list, not the
    // single item about to be removed. The first success returns, so this snapshot stays valid.
    let (into_max, into_slots) = query(world, pos, &mut |container| {
        let slots = (0..container.size())
            .map(|slot| container.get(slot))
            .collect::<Vec<_>>();
        (container.max_stack_size(), slots)
    })
    .unwrap_or((CONTAINER_STACK_LIMIT, Vec::new()));

    for slot in slots {
        let Some(item) = query(world, above, &mut |container| container.get(slot)) else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        let can_take = query(world, above, &mut |container| {
            can_take_item_from_container(
                container,
                &item,
                slot,
                Direction::Down,
                &into_slots,
                into_max,
            )
        })
        .unwrap_or(false);
        if !can_take {
            continue;
        }

        let mut taken = ItemStack::empty();
        let taken_ok = world.silent_container(above, &mut |container| {
            taken = remove_item(container, slot, 1);
        });
        if taken_ok.is_none() {
            continue;
        }
        let mut inserted = None;
        let into_written = world.silent_container(pos, &mut |container| {
            inserted = Some(add_item(container, taken.clone(), None));
        });
        let Some(inserted) = inserted.filter(|_| into_written.is_some()) else {
            world.silent_container(above, &mut |container| container.set(slot, item.clone()));
            continue;
        };

        if inserted.remaining.is_empty() {
            if inserted.was_empty {
                let skip =
                    from_ticked_game_time.is_some_and(|from| entity.ticked_game_time() >= from);
                entity.set_cooldown(MOVE_ITEM_SPEED - i32::from(skip));
            }
            set_changed(world, above);
            return true;
        }

        world.silent_container(above, &mut |container| container.set(slot, item.clone()));
    }
    false
}

/// `Hopper.SUCK_AABB`：方块内 x/z 整格，y 从 11/16 到 2。
///
/// `Hopper.SUCK_AABB`: a full block on x/z, y from 11/16 to 2.
pub fn suck_box(pos: BlockPos) -> Aabb {
    Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y) + 11.0 / 16.0,
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y) + 2.0,
        f64::from(pos.z + 1),
    )
}

fn intersects_suck_box(item: &Entity, pos: BlockPos) -> bool {
    item.bounding_box().intersects(suck_box(pos))
}

/// 漏斗这一格的完整方块。`getEntityInsideCollisionShape` 默认是 `Shapes.block()`。
///
/// The hopper cell as a full cube. `getEntityInsideCollisionShape` defaults to `Shapes.block()`.
fn intersects_block(item: &Entity, pos: BlockPos) -> bool {
    item.bounding_box().intersects(Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    ))
}

/// 上面没有容器时，`getItemsAtAndAbove` 里还活着的物品逐个 `addItem`。
///
/// With no container above, each living item from `getItemsAtAndAbove` is offered to `addItem`.
fn suck_ground_items(world: &mut World, pos: BlockPos, hopper: &mut HopperEntity) -> bool {
    let above = pos.above();
    let above_state = world.block_state(above);
    // 漏斗方块实体总是对齐格子。完整碰撞并且不在 `DOES_NOT_BLOCK_HOPPERS` 里就吸不到。
    // A block hopper is grid-aligned. A full collision outside `DOES_NOT_BLOCK_HOPPERS` blocks the suck.
    if above_state.is_collision_shape_full_block()
        && !above_state.is_tagged(block_tags::DOES_NOT_BLOCK_HOPPERS)
    {
        return false;
    }
    let area = suck_box(pos);
    // 空间索引按 uid 升序给出和吸取盒相交的实体，和官方 `getEntitiesOfClass` 的候选
    // 相同；前一件吸入时可能改了后面的实体，所以只先记 ID，逐个重新读当前状态。
    let mut ids = Vec::new();
    world
        .level()
        .for_each_entity_in(area, &mut |public| ids.push(public.uid));
    for id in ids {
        let stack = world.entity(id).and_then(|entity| {
            if entity.removed || !entity.bounding_box().intersects(area) {
                return None;
            }
            entity.ground().and_then(|item| {
                if item.stack.is_empty() {
                    None
                } else {
                    Some(item.stack.clone())
                }
            })
        });
        let Some(stack) = stack else {
            continue;
        };
        if absorb_stack(world, pos, hopper, id, stack) {
            return true;
        }
    }
    false
}

/// `entityInside` 走的 `tryMoveItems`：先往外送，再只吸正在碰漏斗的这一件。
///
/// `tryMoveItems` from `entityInside`: eject first, then pull in only the entity that is touching.
fn try_move_touching(
    world: &mut World,
    pos: BlockPos,
    hopper: &mut HopperEntity,
    item: &mut Entity,
) {
    if hopper.is_on_cooldown() {
        return;
    }
    if !world.block_state(pos).get(ENABLED) {
        return;
    }
    let mut changed = false;
    if !container_is_empty(world, pos) {
        changed = eject_items(world, pos, hopper);
    }
    if !inventory_full(world, pos) && !item.removed && intersects_suck_box(item, pos) {
        let stack = item
            .ground()
            .map(|ground| ground.stack.clone())
            .unwrap_or_else(ItemStack::empty);
        if !stack.is_empty() {
            changed |= absorb_held(world, pos, hopper, item, stack);
        }
    }
    if changed {
        hopper.set_cooldown(MOVE_ITEM_SPEED);
        set_changed(world, pos);
    }
}

/// `addItem(hopper, itemEntity)`。整堆吸完才返回 true，并丢掉实体。
///
/// `addItem(hopper, itemEntity)`. Returns true only when the whole stack is taken, and then discards it.
fn absorb_stack(
    world: &mut World,
    pos: BlockPos,
    hopper: &mut HopperEntity,
    id: i32,
    stack: ItemStack,
) -> bool {
    let added = insert_into_hopper(world, pos, hopper, stack);
    let Some(added) = added else {
        return false;
    };
    let absorbed = added.remaining.is_empty();
    world.with_entity_mut(id, &mut |entity| {
        if let Some(item) = entity.ground_mut() {
            item.stack = added.remaining.clone();
        }
        if absorbed {
            entity.discard();
        }
    });
    if absorbed {
        world.remove_entity(id);
    }
    absorbed
}

fn absorb_held(
    world: &mut World,
    pos: BlockPos,
    hopper: &mut HopperEntity,
    item: &mut Entity,
    stack: ItemStack,
) -> bool {
    let added = insert_into_hopper(world, pos, hopper, stack);
    let Some(added) = added else {
        return false;
    };
    let absorbed = added.remaining.is_empty();
    if let Some(ground) = item.ground_mut() {
        ground.stack = added.remaining;
    }
    if absorbed {
        item.discard();
    }
    absorbed
}

fn insert_into_hopper(
    world: &mut World,
    pos: BlockPos,
    hopper: &mut HopperEntity,
    stack: ItemStack,
) -> Option<Added> {
    let added = world.silent_container(pos, &mut |container| {
        add_item(container, stack.clone(), None)
    })?;
    // 空漏斗吃进第一件时，`tryMoveInItem` 自己把冷却设成 8。来源不是漏斗，不减 1。
    // An empty hopper sets its own cooldown to 8 on the first insert. The source is not a hopper.
    if added.was_empty && !hopper.is_on_custom_cooldown() {
        hopper.set_cooldown(MOVE_ITEM_SPEED);
    }
    Some(added)
}

/// `getSlots` 加 `isFullContainer`：这些槽都有东西并且都到了堆叠上限。
fn is_full_container(container: &dyn Container, direction: Option<Direction>) -> bool {
    let slots = slots_for_face(container, direction);
    slots.iter().all(|&slot| {
        let stack = container.get(slot);
        stack.count >= item_max(&stack)
    })
}

/// `getSlots`：`WorldlyContainer` 按面取，否则整张表。
fn slots_for_face(container: &dyn Container, direction: Option<Direction>) -> Vec<usize> {
    if container.is_worldly()
        && let Some(direction) = direction
    {
        container
            .slots_for_face(direction)
            .map_or_else(Vec::new, <[usize]>::to_vec)
    } else {
        (0..container.size()).collect()
    }
}

/// `canPlaceItemInContainer`。
fn can_place_item_in_container(
    container: &dyn Container,
    stack: &ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> bool {
    container.can_place_item(slot, stack)
        && !(container.is_worldly()
            && !container.can_place_item_through_face(slot, stack, direction))
}

/// `canTakeItemFromContainer`。`into` 是目标容器的槽位，`into_max` 是它的 `getMaxStackSize()`。
///
/// `canTakeItemFromContainer`. `into` is the destination container's slots and `into_max` is
/// its `getMaxStackSize()`.
fn can_take_item_from_container(
    container: &dyn Container,
    stack: &ItemStack,
    slot: usize,
    direction: Direction,
    into: &[ItemStack],
    into_max: i32,
) -> bool {
    container.can_take_item_into(slot, stack, into, into_max)
        && !(container.is_worldly()
            && !container.can_take_item_through_face(slot, stack, direction))
}

/// `canMergeItems`。
fn can_merge_items(current: &ItemStack, moving: &ItemStack) -> bool {
    current.count <= item_max(current) && same_item_same_components(current, moving)
}

/// `addItem` 的结果。
pub struct Added {
    /// 没放进去、还拿在手上的那一堆。空表示全部放进去了。
    pub remaining: ItemStack,
    /// 目标容器原来整个是空的。往空漏斗里放东西时决定冷却。
    pub was_empty: bool,
}

/// 官方 `HopperBlockEntity.addItem(from, container, stack, direction)`。
///
/// 冷却要用到的来源信息在调用方手里，这里只负责把物品放进目标容器。
pub fn add_item(
    container: &mut dyn Container,
    stack: ItemStack,
    direction: Option<Direction>,
) -> Added {
    let slots = slots_for_face(container, direction);
    let mut remaining = stack;
    let mut was_empty = false;
    for slot in slots {
        if remaining.is_empty() {
            break;
        }
        let inserted = try_move_in_item(container, remaining, slot, direction);
        was_empty |= inserted.was_empty && inserted.success;
        remaining = inserted.remaining;
    }
    Added {
        remaining,
        was_empty,
    }
}

/// `tryMoveInItem` 的结果。
struct Moved {
    remaining: ItemStack,
    success: bool,
    was_empty: bool,
}

/// 官方 `HopperBlockEntity.tryMoveInItem`：往一格放，返回没放进去的部分。
fn try_move_in_item(
    container: &mut dyn Container,
    mut stack: ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> Moved {
    let current = container.get(slot);
    let mut success = false;
    let mut was_empty = false;
    if can_place_item_in_container(container, &stack, slot, direction) {
        was_empty = container.is_empty();
        if current.is_empty() {
            container.set(slot, stack.clone());
            stack = ItemStack::empty();
            success = true;
        } else if can_merge_items(&current, &stack) {
            let space = item_max(&stack) - current.count;
            let count = stack.count.min(space);
            stack.count -= count;
            if stack.count <= 0 {
                stack = ItemStack::empty();
            }
            let mut merged = current;
            merged.count += count;
            container.set(slot, merged);
            success = count > 0;
        }
    }
    Moved {
        remaining: stack,
        success,
        was_empty,
    }
}

/// `BlockEntity.setChanged`：内容变了，记下变化并告诉旁边和隔一格的比较器。
///
/// 双箱的 `CompoundContainer.setChanged` 两半都通知，所以位置由 [`Level::container_positions`]
/// 按合并后的容器给出；每一半用自己那一格的状态，和官方一样。
///
pub fn set_changed(world: &mut World, pos: BlockPos) {
    for changed in world.container_positions(pos) {
        let state = world.block_state(changed);
        if state.is_air() {
            continue;
        }
        // 堆肥桶的容器不是方块实体：官方 `InputContainer` / `OutputContainer` 的 `setChanged`
        // 只加层或清空（在 [`crate::composter`] 里已经做过），再调一次时槽已空或状态没变，
        // 不会通知比较器。等级变化引起的通知走的是 `setBlock`。
        // A composter's container is not a block entity: vanilla `InputContainer` /
        // `OutputContainer.setChanged` only adds a layer or empties it (already done in
        // [`crate::composter`]); a second call finds the slot empty or the state unchanged and
        // notifies no comparator. Level changes reach comparators through `setBlock`.
        if state.is_a(BlockAncestor::ComposterBlock) {
            continue;
        }
        world.update_neighbour_for_output_signal(changed, state.block());
    }
}

/// 物品没变的安静刻通知。只给证明测试用：那个开关能省掉这次通知，
/// 用来逐刻比较效果和包，见 `block_entity_sleep` 的证明测试。
///
/// The quiet, item-unchanged notification. Only used by the proof test that
/// switches it off to compare effects and packets tick by tick.
pub(crate) fn quiet_set_changed(world: &mut World, pos: BlockPos, quiet_only: bool) {
    #[cfg(test)]
    if quiet_only && !crate::block_entity_sleep::quiet_changed_enabled() {
        return;
    }
    let _ = quiet_only;
    set_changed(world, pos);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{BlockState, BlockTicks, state::ENABLED, testing::TestLevel};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn empty(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    fn hopper_with_ten_dirt() -> Vec<ItemStack> {
        let mut slots = empty(5);
        slots[0] = ItemStack::new(items::DIRT, 10);
        slots
    }

    /// 放好一格漏斗和它的方块实体。
    fn placed_hopper<'a>(
        level: &'a mut TestLevel,
        ticks: &'a mut BlockTicks,
        pos: BlockPos,
    ) -> World<'a> {
        level.set(pos, BlockState(blocks::HOPPER));
        let mut world = World::new(level, ticks);
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
        world
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        query(world, pos, &mut |container| container.get(index)).expect("这一格应该是容器")
    }

    fn cooldown(world: &World, pos: BlockPos) -> i32 {
        world
            .block_entity::<HopperEntity>(pos)
            .expect("漏斗实体应该还在")
            .cooldown_time()
    }

    #[test]
    fn a_hopper_ejects_one_item_then_waits_eight_ticks() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(POS, hopper_with_ten_dirt());
        level.set_container(below, empty(27));
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 1));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 9));
        assert_eq!(cooldown(&world, POS), MOVE_ITEM_SPEED);

        // 冷却 8 刻：第 2 到第 8 刻都不动，第 9 刻再送一个。
        for now in 1..8 {
            world.ticks_mut().set_time(now);
            world.tick_block_entities();
        }
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 1));
        world.ticks_mut().set_time(8);
        world.tick_block_entities();
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 2));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 8));
    }

    #[test]
    fn a_locked_hopper_does_nothing() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER).with(ENABLED, false));
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(POS, hopper_with_ten_dirt());
        level.set_container(below, empty(27));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block_entity(Box::new(HopperEntity::new(POS)));

        world.tick_block_entities();
        assert!(slot(&mut world, below, 0).is_empty());
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 10));
    }

    #[test]
    fn a_hopper_sucks_one_item_from_the_container_above() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(above, BlockState(blocks::CHEST));
        level.set_container(POS, empty(5));
        let mut chest = empty(27);
        chest[3] = ItemStack::new(items::DIRT, 5);
        level.set_container(above, chest);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 1));
        assert_eq!(slot(&mut world, above, 3), ItemStack::new(items::DIRT, 4));
    }

    #[test]
    fn a_full_hopper_does_not_suck() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(above, BlockState(blocks::CHEST));
        level.set_container(POS, vec![ItemStack::new(items::STONE, 64); 5]);
        let mut chest = empty(27);
        chest[0] = ItemStack::new(items::DIRT, 5);
        level.set_container(above, chest);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, above, 0), ItemStack::new(items::DIRT, 5));
    }

    #[test]
    fn the_comparator_reads_the_hopper_contents() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        let mut slots = empty(5);
        slots[0] = ItemStack::new(items::STONE, 64);
        level.set_container(POS, slots);
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut level, &mut ticks);

        // 5 格里的 1 摞满石头：1/5 * 14 = 2.8，向下取整再加一。
        assert_eq!(
            world
                .block_state(POS)
                .analog_output_signal(&world, POS, Direction::Up),
            3
        );
    }
}
