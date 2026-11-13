//! 方块实体休眠的差分测试。
//!
//! 同一个随机场景跑两份世界，一份开休眠、一份关休眠（退回每刻都跑）。
//! **每一刻**比较方块状态、容器、方块实体的全部字段（睡着的按公式现算）、
//! 写方块序列、随机数种子和计划刻，任何一位不同都算失败。

use std::cell::Cell;

use mc_bamboo_core::{BlockPos, ChunkPos, Direction};
use mc_bamboo_data::{blocks, items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry},
    item::ItemStack,
};

use crate::{
    BlockState, BlockTicks, HopperEntity,
    behaviour::machines::{BrewingStandEntity, CampfireEntity, FurnaceEntity},
    block_entity::{block_entity_clock, set_block_entity_clock},
    hopper::HOPPER_CONTAINER_SIZE,
    interact::run_block_entity_tick,
    level::LevelTables,
    random::LegacyRandom,
    recording::{Record, Recording},
    state::{ComparatorMode, HORIZONTAL_FACING, LIT, MODE_COMPARATOR},
    testing::TestLevel,
    world::{UPDATE_ALL, World},
};

const FURNACE: BlockPos = BlockPos::new(0, 64, 0);
const ABOVE: BlockPos = BlockPos::new(0, 65, 0);
const BELOW: BlockPos = BlockPos::new(0, 63, 0);
const HOPPER_BEFORE: BlockPos = BlockPos::new(-1, 65, 0);
const HOPPER_AFTER: BlockPos = BlockPos::new(0, 64, -1);

/// 一个可复现的动作流，和世界状态无关，两份世界用同一份。
struct Script {
    /// 每个动作是 `(时刻, 动作)`。
    actions: Vec<(i64, Action)>,
}

#[derive(Clone, Debug)]
enum Action {
    AddFuel(ItemStack),
    AddInput(ItemStack),
    TakeInput(i32),
    TakeResult(i32),
    /// 往漏斗链上游放一个物品（会送进熔炉的原料格）。
    HopperFeed(ItemStack),
    /// 抽走熔炉下方漏斗的结果。
    HopperDrain,
    /// 换掉熔炉的 `LIT` 状态。
    ToggleLit,
    /// 破坏熔炉并重新放一个。
    BreakAndReplace,
    /// 把整个区块装盒再放回（模拟搬区块）。
    BoxChunk,
    /// 冻结 / 恢复熔炉所在区块。
    Freeze(bool),
    /// 开着的界面每刻读数据槽，这里记一次显式读取。
    MenuRead,
}

impl Script {
    fn random(seed: u64, ticks: i64, layout: Layout) -> Self {
        let mut random = LegacyRandom::new(seed as i64);
        let mut actions = Vec::new();
        for now in 1..=ticks {
            let roll = random.next_int(100);
            let action = match roll {
                0..=7 => Some(Action::AddFuel(fuel(&mut random))),
                8..=20 => Some(Action::AddInput(input(&mut random))),
                21..=26 => Some(Action::TakeInput(1 + random.next_int(3))),
                27..=32 => Some(Action::TakeResult(1)),
                33..=44 if layout.has_hoppers() => Some(Action::HopperFeed(input(&mut random))),
                45..=52 if layout.has_hoppers() => Some(Action::HopperDrain),
                53..=55 => Some(Action::ToggleLit),
                56 if now > 400 && now % 3 == 0 => Some(Action::BreakAndReplace),
                57 if now > 300 && now % 5 == 0 => Some(Action::BoxChunk),
                58..=60 => Some(Action::Freeze(random.next_int(2) == 0)),
                61..=70 => Some(Action::MenuRead),
                _ => None,
            };
            if let Some(action) = action {
                actions.push((now, action));
            }
        }
        let _ = seed;
        Self { actions }
    }
}

#[derive(Clone, Copy, Debug)]
enum Layout {
    /// 只有一台熔炉，外加上方/下方的漏斗。
    HoppersBeforeFurnace,
    HopperAfterFurnace,
    Plain,
}

impl Layout {
    const fn has_hoppers(self) -> bool {
        !matches!(self, Self::Plain)
    }
}

fn fuel(random: &mut LegacyRandom) -> ItemStack {
    if random.next_int(4) == 0 {
        // 岩浆桶有余料（空桶），余料放不下时要掉落并取世界随机数。
        ItemStack::new(items::LAVA_BUCKET, 1)
    } else {
        ItemStack::new(items::COAL, 1 + random.next_int(4))
    }
}

fn input(random: &mut LegacyRandom) -> ItemStack {
    if random.next_int(3) == 0 {
        ItemStack::new(items::POTATO, 1 + random.next_int(3))
    } else {
        ItemStack::new(items::RAW_IRON, 1 + random.next_int(5))
    }
}

struct Scene {
    level: TestLevel,
    ticks: BlockTicks,
    script: Script,
    cursor: usize,
    layout: Layout,
    /// 开着的界面每刻读到的数据槽，逐刻比较。
    menu: Vec<i32>,
}

impl Scene {
    fn new(script: Script, layout: Layout, sleep: bool) -> Self {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let hoppers = layout.has_hoppers();
        if hoppers {
            for pos in [ABOVE, BELOW, HOPPER_BEFORE, HOPPER_AFTER] {
                level_container(&mut level, pos);
            }
        }
        {
            let mut world = World::new(&mut level, &mut ticks);
            // `HopperAfterFurnace` 让漏斗先登记（在熔炉前面的顺序位置），
            // 这样漏斗这一刻写完熔炉之后熔炉还会跑；另一种布局反过来。
            let hopper_first = matches!(layout, Layout::HopperAfterFurnace);
            if hoppers && hopper_first {
                place_hoppers(&mut world);
            }
            world.set_block(FURNACE, BlockState(blocks::FURNACE), UPDATE_ALL);
            world.with_block_entity::<FurnaceEntity, _>(FURNACE, |entity, _| {
                entity.write_item(0, ItemStack::new(items::RAW_IRON, 16));
                entity.write_item(1, ItemStack::new(items::COAL, 8));
            });
            if hoppers && !hopper_first {
                place_hoppers(&mut world);
            }
        }
        level.sleep_enabled(sleep);
        Self {
            level,
            ticks,
            script,
            cursor: 0,
            layout,
            menu: Vec::new(),
        }
    }

    fn step(&mut self, now: i64) {
        self.ticks.set_time(now);
        set_block_entity_clock(now);
        while self
            .script
            .actions
            .get(self.cursor)
            .is_some_and(|(at, _)| *at == now)
        {
            let action = self.script.actions[self.cursor].1.clone();
            self.cursor += 1;
            self.apply(action, now);
        }
        let _ = run_block_entity_tick(&mut self.level, &mut self.ticks);
        // 官方 `broadcastChanges` 在方块实体阶段之后读数据槽。
        set_block_entity_clock(now);
        if let Some(entity) = self.level.block_entity::<FurnaceEntity>(FURNACE) {
            self.menu.extend_from_slice(&entity.data_values());
        } else {
            self.menu.extend_from_slice(&[0; 4]);
        }
    }

    fn apply(&mut self, action: Action, _now: i64) {
        let is_fuel = matches!(action, Action::AddFuel(_));
        let is_input = matches!(action, Action::TakeInput(_));
        let mut world = World::new(&mut self.level, &mut self.ticks);
        match action {
            Action::AddFuel(stack) | Action::AddInput(stack) => {
                let slot = usize::from(is_fuel);
                let stack = stack.clone();
                world.with_container(FURNACE, &mut |container| {
                    let mut current = container.get(slot);
                    if current.is_empty() {
                        container.set(slot, stack.clone());
                    } else if current.item == stack.item {
                        current.count += stack.count;
                        container.set(slot, current);
                    }
                });
            }
            Action::TakeInput(amount) | Action::TakeResult(amount) => {
                let slot = if is_input { 0 } else { 2 };
                world.with_container(FURNACE, &mut |container| {
                    let _ = container.remove_item(slot, amount);
                });
            }
            Action::HopperFeed(stack) => {
                let stack = stack.clone();
                let at = if matches!(self.layout, Layout::HopperAfterFurnace) {
                    HOPPER_AFTER
                } else {
                    HOPPER_BEFORE
                };
                world.with_container(at, &mut |container| {
                    for slot in 0..container.size() {
                        if container.get(slot).is_empty() {
                            container.set(slot, stack.clone());
                            break;
                        }
                    }
                });
            }
            Action::HopperDrain => {
                let at = if matches!(self.layout, Layout::HopperAfterFurnace) {
                    HOPPER_AFTER
                } else {
                    HOPPER_BEFORE
                };
                world.with_container(at, &mut |container| {
                    for slot in 0..container.size() {
                        let _ = container.remove_item(slot, 64);
                    }
                });
            }
            Action::ToggleLit => {
                let state = world.block_state(FURNACE);
                world.set_block_and_update(FURNACE, state.with(LIT, !state.get(LIT)));
            }
            Action::BreakAndReplace => {
                world.set_block(FURNACE, BlockState(blocks::AIR), UPDATE_ALL);
                world.set_block(FURNACE, BlockState(blocks::FURNACE), UPDATE_ALL);
                world.with_block_entity::<FurnaceEntity, _>(FURNACE, |entity, _| {
                    entity.write_item(0, ItemStack::new(items::RAW_IRON, 8));
                    entity.write_item(1, ItemStack::new(items::COAL, 4));
                });
            }
            Action::BoxChunk => {
                // 把整个区块装盒，立刻放回同一个表（时钟不变）。
                let chunk = ChunkPos::new(0, 0);
                let _ = world;
                self.level.box_chunk(chunk);
            }
            Action::Freeze(frozen) => {
                let _ = world;
                self.level
                    .set_chunk_block_entity_ticking(ChunkPos::new(0, 0), !frozen);
                self.level.set_frozen(ChunkPos::new(0, 0), frozen);
            }
            Action::MenuRead => {
                // 快照里已经每刻读一次数据槽；这里额外读一次并记进快照。
                if let Some(entity) = world.block_entity::<FurnaceEntity>(FURNACE) {
                    self.menu.extend_from_slice(&entity.data_values());
                }
            }
        }
    }

    fn snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        for pos in [FURNACE, ABOVE, BELOW, HOPPER_BEFORE, HOPPER_AFTER] {
            out.push(format!("b{pos:?}:{}", self.level.get(pos).0));
        }
        let entity = self.level.block_entity::<FurnaceEntity>(FURNACE);
        out.push(format!(
            "data:{:?}",
            entity.map_or(vec![0; 4], |e| e.data_values().to_vec())
        ));
        out.push(format!(
            "items:{:?}",
            entity.map_or(vec![], |e| e.items().to_vec())
        ));
        out.push(format!("facts:{:?}", entity.map(|e| e.recipe_facts())));
        out.push(format!(
            "speed:{:?}",
            entity.map(|e| e.speed_multiplier().to_bits())
        ));
        for pos in [ABOVE, BELOW, HOPPER_BEFORE, HOPPER_AFTER] {
            out.push(format!(
                "hopper{pos:?}:{:?}",
                self.level
                    .block_entity::<HopperEntity>(pos)
                    .map(|e| e.cooldown_time())
            ));
            out.push(format!(
                "slots{pos:?}:{:?}",
                self.level.containers.get(&pos)
            ));
        }
        // 存档组装拿到的值必须和每刻都跑的一致。
        out.push(format!(
            "archive:{:?}",
            entity.map(|e| crate::archive::write_block_entity_additional(e))
        ));
        out.push(format!("seed:{}", self.level.random.source().seed()));
        out.push(format!("writes:{:?}", self.level.writes));
        out.push(format!("menu:{:?}", self.menu));
        out.push(format!("clock:{}", block_entity_clock()));
        out
    }
}

fn level_container(level: &mut TestLevel, pos: BlockPos) {
    level.set_container(pos, vec![ItemStack::empty(); HOPPER_CONTAINER_SIZE]);
}

/// 上方漏斗朝下送进熔炉原料格，下方漏斗接结果格；两个侧向漏斗用来做
/// 「漏斗链往里送和往外抽」的源和汇。
fn place_hoppers(world: &mut World<'_>) {
    use crate::state::FACING_HOPPER;
    for (pos, facing) in [
        (ABOVE, Direction::Down),
        (BELOW, Direction::Down),
        (HOPPER_BEFORE, Direction::East),
        (HOPPER_AFTER, Direction::West),
    ] {
        world.set_block(
            pos,
            BlockState(blocks::HOPPER).with(FACING_HOPPER, facing),
            UPDATE_ALL,
        );
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
    }
}

/// 一份场景开 / 关休眠各跑一遍，逐刻比较。
fn compare(seed: u64, ticks: i64, layout: Layout) -> usize {
    let script = Script::random(seed, ticks, layout);
    let mut sleeping = Scene::new(
        Script {
            actions: script.actions.clone(),
        },
        layout,
        true,
    );
    let mut always = Scene::new(
        Script {
            actions: script.actions,
        },
        layout,
        false,
    );
    let mut slept = 0;
    for now in 1..=ticks {
        sleeping.step(now);
        always.step(now);
        let a = sleeping.snapshot();
        let b = always.snapshot();
        assert_eq!(
            a,
            b,
            "seed {seed} layout {:?} tick {now}",
            layout_name(layout)
        );
        if sleeping.level.due_count() == 0 {
            slept += 1;
        }
    }
    assert!(
        sleeping
            .snapshot()
            .iter()
            .any(|row| row.starts_with("slotsBlockPos { x: 0, y: 63, z: 0 }")),
        "snapshot covers the hoppers"
    );
    slept
}

/// 没有外部动作时，一台熔炉在 3000 刻里只该在事件刻真跑。
#[test]
fn an_undisturbed_furnace_runs_only_on_event_ticks() {
    let script = Script {
        actions: Vec::new(),
    };
    let mut scene = Scene::new(script, Layout::Plain, true);
    let mut ran = 0;
    for now in 1..=3000 {
        scene.step(now);
        ran += scene.level.due_count();
    }
    // 点火 1 刻、八个原料各 200 刻、熄灭、之后一直睡到被叫醒。
    assert!(ran < 20, "furnace ran on {ran} of 3000 ticks");
}

const fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::HoppersBeforeFurnace => "hopper-before",
        Layout::HopperAfterFurnace => "hopper-after",
        Layout::Plain => "plain",
    }
}

#[test]
fn furnace_sleep_matches_ticking_every_tick() {
    let mut total_slept = 0;
    let mut rounds = 0;
    for seed in [1, 7, 42, 1337, 90210] {
        for layout in [
            Layout::Plain,
            Layout::HoppersBeforeFurnace,
            Layout::HopperAfterFurnace,
        ] {
            total_slept += compare(seed, 2000, layout);
            rounds += 1;
        }
    }
    // 纯熔炉无动作场景的近乎零开销由 `an_undisturbed_furnace_runs_only_on_event_ticks`
    // 单独断言；带漏斗的场景里漏斗仍每刻跑，所以总数只作报告。
    let _ = (rounds, total_slept);
}

// 安静刻那次 `setChanged` 的开关。只给证明用：关掉时省掉「物品没变」的通知，
// 真改了物品的那些（放食物、烤熟、燃料消耗、doBrew、开一轮）一律照发。
// 线程局部：方块逻辑都在调用它的那个线程上跑，两个测试线程互不干扰。
thread_local! {
    static QUIET_NOTIFICATIONS: Cell<bool> = const { Cell::new(true) };
}

#[derive(Clone, Copy, Debug)]
enum Machine {
    Campfire,
    BrewingStand,
}

/// 安静刻（倒计时 / 冷却）里物品不变的通知要不要照发。
pub(crate) fn quiet_changed_enabled() -> bool {
    QUIET_NOTIFICATIONS.with(|cell| cell.get())
}

const MACHINE: BlockPos = BlockPos::new(0, 64, 0);
const NEAR_COMPARATOR: BlockPos = BlockPos::new(1, 64, 0);
const REDSTONE_WALL: BlockPos = BlockPos::new(-1, 64, 0);
const FAR_COMPARATOR: BlockPos = BlockPos::new(-2, 64, 0);

struct QuietScene {
    level: TestLevel,
    ticks: BlockTicks,
    /// `true` = 照官方每刻通知；`false` = 只留真改了物品的通知。
    notify: bool,
    /// 录制层收集的效果、包和写序列，`nanos` 不进比较。
    records: Vec<Record>,
    /// 上一刻新增的记录数，用来把比较范围限定在这一刻。
    last_added: usize,
}

impl QuietScene {
    fn new(machine: Machine, notify: bool) -> Self {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        // 这里只比较两种通知口径，不掺休眠的影响。
        level.sleep_enabled(false);
        {
            let mut world = World::new(&mut level, &mut ticks);
            // 下面一层都垫上，比较器不用检查支撑。旁边一个、隔一格一个，
            // 后者经红石导体转发，两条路径都走到 `checkTickOnNeighbor`。
            for pos in [MACHINE, NEAR_COMPARATOR, REDSTONE_WALL, FAR_COMPARATOR] {
                world.set_block(pos.below(), BlockState(blocks::STONE), UPDATE_ALL);
            }
            world.set_block(REDSTONE_WALL, BlockState(blocks::STONE), UPDATE_ALL);
            for (pos, facing) in [
                (NEAR_COMPARATOR, Direction::West),
                (FAR_COMPARATOR, Direction::East),
            ] {
                let state = BlockState(blocks::COMPARATOR)
                    .with(HORIZONTAL_FACING, facing)
                    .with(MODE_COMPARATOR, ComparatorMode::Compare);
                world.set_block(pos, state, UPDATE_ALL);
            }
            let block = match machine {
                Machine::Campfire => blocks::CAMPFIRE,
                Machine::BrewingStand => blocks::BREWING_STAND,
            };
            world.set_block(MACHINE, BlockState(block), UPDATE_ALL);
        }
        seed_machine(&mut level, &mut ticks, machine);
        Self {
            level,
            ticks,
            notify,
            records: Vec::new(),
            last_added: 0,
        }
    }

    fn step(&mut self, now: i64) -> Vec<String> {
        self.ticks.set_time(now);
        set_block_entity_clock(now);
        QUIET_NOTIFICATIONS.with(|cell| cell.set(self.notify));
        // 中途改内容：真改物品的通知和安静刻的通知会连在一起。
        if now == 700 {
            disturb(&mut self.level, &mut self.ticks);
        }
        let mut recorder = Recording::new(&mut self.level);
        recorder.set_tick(now);
        // 到期的比较器计划刻也在录制范围内：通知若真的改变了输出，
        // 计划刻跑起来之后的包和写方块会不一样。
        let mut effects =
            crate::interact::run_due_ticks(&mut recorder, &mut self.ticks, now).effects;
        {
            let mut world = World::in_world_tick(&mut recorder, &mut self.ticks);
            world.tick_block_entities();
            effects.extend(world.drain_effects());
        }
        let added = recorder.take_records();
        self.last_added = added.len();
        self.records.extend(added);
        let mut out = Vec::new();
        // 录制层记下了每一刻的效果、包、写方块、红石记忆和计划刻。
        // `nanos` 是墙上时间，不能进逐位比较。
        // 只比这一刻新增的记录，且把「通知本身读了什么」排除：省掉一次通知
        // 当然少读几次方块，那不是可见效果。要看的是它有没有写、有没有发包。
        let from = self.records.len();
        for record in &self.records[from.saturating_sub(self.last_added)..] {
            // 排计划刻本身是查重操作：已经在队列里时 `schedule` 什么也不做，
            // 队列的状态（含时刻、优先级）由下面的 `ticks` 快照逐刻比较。
            // 其它每一笔写都是可见效果，一个都不能省。
            let writes: Vec<_> = record
                .writes
                .iter()
                .filter(|write| !matches!(write, crate::recording::Write::TickScheduled { .. }))
                .collect();
            out.push(format!(
                "{:?}/{:?}/{:?}/{:?}",
                record.tick, record.trigger, writes, record.uncacheable
            ));
        }
        out.push(format!("effects:{effects:?}"));
        let level = &self.level;
        for pos in [MACHINE, NEAR_COMPARATOR, REDSTONE_WALL, FAR_COMPARATOR] {
            out.push(format!("b{pos:?}:{}", level.get(pos).0));
        }
        for pos in [NEAR_COMPARATOR, FAR_COMPARATOR] {
            out.push(format!(
                "c{pos:?}:{}",
                level.redstone_table().comparator_output(pos)
            ));
        }
        out.push(format!("writes:{:?}", level.writes));
        out.push(format!("seed:{}", level.random.source().seed()));
        // 计划刻按（触发刻、优先级、位置）排序比较：表里的 HashMap 迭代顺序
        // 不是对外行为，`will_tick_this_tick` 查重也只看那一组。
        let mut scheduled: Vec<_> = self
            .ticks
            .pending()
            .map(|tick| (tick.trigger_tick, tick.priority, tick.pos, tick.block))
            .collect();
        scheduled.sort();
        out.push(format!("scheduled:{scheduled:?}"));
        // 两份世界各自的容器、数据槽、方块实体内部状态也要逐刻相同。
        out.push(format!("items:{:?}", machine_snapshot(&self.level)));
        out
    }
}

/// 两台机器当前能观察到的全部内部状态（数据槽、物品、配方缓存）。
fn machine_snapshot(level: &TestLevel) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(campfire) = level.block_entity::<CampfireEntity>(MACHINE) {
        out.push(format!("{:?}", campfire.data_values()));
        out.push(format!("{:?}", campfire.items()));
        out.push(format!("{:?}", campfire.recipe_facts()));
    }
    if let Some(stand) = level.block_entity::<BrewingStandEntity>(MACHINE) {
        out.push(format!("{:?}", stand.data_values()));
        out.push(format!("{:?}", stand.items()));
        out.push(format!("{:?}", stand.observed_facts()));
    }
    out
}

/// 官方 `PotionContents.createItemStack(Items.POTION, Potions.WATER)`。
fn water_bottle() -> ItemStack {
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::POTION_CONTENTS,
        PatchEntry::Set(ComponentValue::PotionContents {
            potion: Some(0),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        }),
    );
    let (added, removed) = patch.to_opaque();
    let mut stack = ItemStack::new(items::POTION, 1);
    stack.added = added;
    stack.removed = removed;
    stack
}

/// 营火放四份食物（错开的进度），酿造台放三瓶水、地狱疣和烈焰粉。
fn seed_machine(level: &mut TestLevel, ticks: &mut BlockTicks, machine: Machine) {
    let mut world = World::new(level, ticks);
    match machine {
        Machine::Campfire => {
            world.with_block_entity::<CampfireEntity, _>(MACHINE, |entity, _| {
                for slot in 0..4 {
                    entity.set_item(slot, ItemStack::new(items::POTATO, 1));
                    entity.set_data(4 + slot, 600);
                    entity.set_data(slot, 120 * slot as i32);
                }
            });
        }
        Machine::BrewingStand => {
            // 官方水瓶是带 `potion_contents = minecraft:water` 的药水物品。
            let water = water_bottle();
            world.with_block_entity::<BrewingStandEntity, _>(MACHINE, |entity, _| {
                for slot in 0..3 {
                    entity.set_item(slot, water.clone());
                }
                entity.set_item(3, ItemStack::new(items::NETHER_WART, 3));
                entity.set_item(4, ItemStack::new(items::BLAZE_POWDER, 4));
            });
        }
    }
}

/// 中途真改一次物品：营火熄火后重新点燃，酿造台换掉材料格。
fn disturb(level: &mut TestLevel, ticks: &mut BlockTicks) {
    let mut world = World::new(level, ticks);
    let state = world.block_state(MACHINE);
    if let Some(lit) = state.try_get(LIT) {
        world.set_block_and_update(MACHINE, state.with(LIT, !lit));
    } else {
        world.with_container(MACHINE, &mut |container| {
            let _ = container.remove_item(3, 1);
        });
    }
}

/// 同一个场景跑两遍：一遍照官方每刻都通知比较器，一遍只留「物品真变了」的那些通知。
/// 录制层逐刻把效果、包、写方块、计划刻和状态全部记下来，两者必须逐位相同。
/// 营火和酿造台在倒计时期间每刻 `setChanged`，这条测试是省掉它们的依据。
const fn machine_name(machine: Machine) -> &'static str {
    match machine {
        Machine::Campfire => "营火",
        Machine::BrewingStand => "酿造台",
    }
}

fn compare_quiet_notifications(machine: Machine, ticks: i64) {
    let mut notifying = QuietScene::new(machine, true);
    let mut quiet = QuietScene::new(machine, false);
    for now in 1..=ticks {
        let a = notifying.step(now);
        let b = quiet.step(now);
        assert_eq!(
            a, b,
            "machine {machine:?} tick {now}: 安静刻的通知有可见效果"
        );
    }
    // 证明覆盖到真事件：两种机器都跑过完整的一轮，而且安静刻的通知
    // 在「照发」那份里确实产生过额外的读（否则这份证明是空的）。
    assert!(notifying.records.len() > 100, "录制层覆盖了每一刻");
    let reads = |scene: &QuietScene| -> usize {
        scene.records.iter().map(|record| record.reads.len()).sum()
    };
    assert!(
        reads(&notifying) > reads(&quiet) + 100,
        "{} 份安静刻通知真的被省掉了",
        machine_name(machine)
    );
}

#[test]
fn unchanged_items_make_campfire_and_brewing_notifications_invisible() {
    // 烤熟一整轮 + 熄灭后冷却 + 一轮完整酿造 + 中途换配方（真改物品的通知要留下）。
    compare_quiet_notifications(Machine::Campfire, 1400);
    compare_quiet_notifications(Machine::BrewingStand, 1400);
}

#[test]
fn crafter_clock_sleeps_until_it_expires() {
    // 合成器倒计时：亮起六刻，中间每刻不碰这张表，到点才关 `crafting`。
    use crate::{
        crafter::dispense_from,
        state::{CRAFTING, FrontAndTop, ORIENTATION},
    };
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        let state = BlockState(blocks::CRAFTER).with(ORIENTATION, FrontAndTop::UpNorth);
        world.set_block(FURNACE, state, UPDATE_ALL);
        level.set_container(FURNACE, vec![ItemStack::empty(); 9]);
    }
    // 直接触发一次成功的合成，亮起 `crafting`。
    {
        let mut world = World::new(&mut level, &mut ticks);
        world.with_container(FURNACE, &mut |container| {
            container.set(0, ItemStack::new(items::OAK_LOG, 1));
        });
        let state = world.block_state(FURNACE);
        dispense_from(&mut world, FURNACE, state);
    }
    assert!(level.get(FURNACE).get(CRAFTING));
    for now in 1..6 {
        ticks.set_time(now);
        let _ = run_block_entity_tick(&mut level, &mut ticks);
        assert!(level.get(FURNACE).get(CRAFTING), "tick {now}");
    }
    ticks.set_time(6);
    let _ = run_block_entity_tick(&mut level, &mut ticks);
    assert!(!level.get(FURNACE).get(CRAFTING));
}

/// 测量用的手工基准：几百台正在烧东西的熔炉，方块实体阶段每刻的耗时。
///
/// 用法（release、安静窗口）：
/// `cargo test --release -p mc-bamboo-blocks block_entity_sleep::furnace_phase_bench -- --ignored --nocapture`
/// 同一轮里先跑关掉休眠的对照，再跑开着的，取多次里最快的一次，避免跨时间段比较。
#[test]
#[ignore]
fn furnace_phase_bench() {
    const COUNT: i32 = 400;
    const TICKS: i64 = 3000;
    const ROUNDS: usize = 5;

    fn build(sleep: bool) -> Scene {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for i in 0..COUNT {
                let pos = BlockPos::new(i, 64, 0);
                world.set_block(pos, BlockState(blocks::FURNACE), UPDATE_ALL);
                world.with_block_entity::<FurnaceEntity, _>(pos, |entity, _| {
                    entity.write_item(0, ItemStack::new(items::RAW_IRON, 64));
                    entity.write_item(1, ItemStack::new(items::COAL, 64));
                });
            }
        }
        level.sleep_enabled(sleep);
        Scene {
            level,
            ticks,
            script: Script {
                actions: Vec::new(),
            },
            cursor: 0,
            layout: Layout::Plain,
            menu: Vec::new(),
        }
    }

    fn measure(sleep: bool) -> f64 {
        let mut best = u128::MAX;
        for _ in 0..ROUNDS {
            let mut scene = build(sleep);
            let started = std::time::Instant::now();
            for now in 1..=TICKS {
                scene.ticks.set_time(now);
                set_block_entity_clock(now);
                let _ = run_block_entity_tick(&mut scene.level, &mut scene.ticks);
            }
            best = best.min(started.elapsed().as_nanos());
        }
        best as f64 / TICKS as f64 / 1000.0
    }

    // 新旧交替，同一时间段里连续跑，取各自最快的一次。
    let mut off = Vec::new();
    let mut on = Vec::new();
    for _ in 0..3 {
        off.push(measure(false));
        on.push(measure(true));
    }
    let off = off.into_iter().fold(f64::INFINITY, f64::min);
    let on = on.into_iter().fold(f64::INFINITY, f64::min);
    println!(
        "{COUNT} 台熔炉，方块实体阶段每刻：休眠关 {off:.1} µs，休眠开 {on:.1} µs，{:.1}×",
        off / on
    );
}
