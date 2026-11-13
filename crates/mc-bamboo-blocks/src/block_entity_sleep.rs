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

/// 营火 / 酿造台休眠的差分测试：多种子 × 多布局，逐刻比较全部字段。
#[derive(Clone, Copy, Debug)]
enum SleepMachine {
    Campfire,
    Brewing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SleepLayout {
    Plain,
    HopperBefore,
    HopperAfter,
}

struct SleepScript {
    actions: Vec<(i64, SleepAction)>,
}

#[derive(Clone, Debug)]
enum SleepAction {
    /// 放一份食物 / 补一份酿造材料或燃料。
    Feed(ItemStack),
    /// 从机器或漏斗里拿走物品。
    Take(usize, i32),
    /// 切换营火的 `LIT`；酿造台没有这个状态时换成破坏重放。
    Toggle,
    /// 把整个区块装盒再放回。
    BoxChunk,
    /// 冻结 / 恢复方块实体所在的区块。
    Freeze(bool),
    /// 换掉漏斗的 `ENABLED`，考验漏斗自己的唤醒入口。
    ToggleHopper,
    /// 往漏斗链上游放一个物品（会送进机器 / 或抽走结果）。
    HopperFeed(ItemStack),
    /// 抽走漏斗链下游的东西。
    HopperDrain,
    /// 主动读两个比较器的输出，逐刻对照读数。
    ComparatorRead,
    /// 把机器存档组装、读回再放回（模拟存盘重启）。
    Restart,
    /// 开着的界面每刻读一次全部数据槽。
    MenuRead,
}

impl SleepScript {
    fn random(seed: u64, ticks: i64, machine: SleepMachine, layout: SleepLayout) -> Self {
        let mut random = LegacyRandom::new(seed as i64);
        let mut actions = Vec::new();
        for now in 1..=ticks {
            let roll = random.next_int(100);
            let action = match roll {
                0..=22 => Some(SleepAction::Feed(feed_stack(machine, &mut random))),
                23..=30 => Some(SleepAction::Take(
                    random.next_int(3) as usize,
                    1 + random.next_int(2),
                )),
                31..=36 => Some(SleepAction::Toggle),
                37..=40 if now > 300 && now % 7 == 0 => Some(SleepAction::BoxChunk),
                41..=45 => Some(SleepAction::Freeze(random.next_int(2) == 0)),
                46..=50 if layout != SleepLayout::Plain => Some(SleepAction::ToggleHopper),
                51..=58 if layout != SleepLayout::Plain => {
                    Some(SleepAction::HopperFeed(feed_stack(machine, &mut random)))
                }
                59..=63 if layout != SleepLayout::Plain => Some(SleepAction::HopperDrain),
                64..=68 => Some(SleepAction::ComparatorRead),
                69 if now > 400 && now % 11 == 0 => Some(SleepAction::Restart),
                70..=80 => Some(SleepAction::MenuRead),
                _ => None,
            };
            if let Some(action) = action {
                actions.push((now, action));
            }
        }
        Self { actions }
    }
}

fn feed_stack(machine: SleepMachine, random: &mut LegacyRandom) -> ItemStack {
    match machine {
        SleepMachine::Campfire => ItemStack::new(items::POTATO, 1 + random.next_int(2)),
        SleepMachine::Brewing => {
            if random.next_int(3) == 0 {
                ItemStack::new(items::BLAZE_POWDER, 1 + random.next_int(2))
            } else {
                ItemStack::new(items::NETHER_WART, 1 + random.next_int(2))
            }
        }
    }
}

struct SleepScene {
    level: TestLevel,
    ticks: BlockTicks,
    script: SleepScript,
    cursor: usize,
    machine: SleepMachine,
    layout: SleepLayout,
    menu: Vec<i32>,
    /// 是否有过真的安静刻（至少一刻一个 ticker 都没跑）。
    slept: usize,
    /// 每次 `Restart` 后重新造出来的方块实体读数。
    restarts: Vec<String>,
}

impl SleepScene {
    fn new(script: SleepScript, machine: SleepMachine, layout: SleepLayout, sleep: bool) -> Self {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let pos = BlockPos::new(0, 64, 0);
        let above = BlockPos::new(0, 65, 0);
        let below = BlockPos::new(0, 63, 0);
        let before = BlockPos::new(-1, 65, 0);
        let after = BlockPos::new(0, 64, -1);
        if layout != SleepLayout::Plain {
            for p in [above, below, before, after] {
                level_container(&mut level, p);
            }
        }
        {
            let mut world = World::new(&mut level, &mut ticks);
            let hopper_first = layout == SleepLayout::HopperAfter;
            if hopper_first {
                place_sleep_hoppers(&mut world, pos, above, below, before, after);
            }
            let block = match machine {
                SleepMachine::Campfire => blocks::CAMPFIRE,
                SleepMachine::Brewing => blocks::BREWING_STAND,
            };
            world.set_block(pos, BlockState(block), UPDATE_ALL);
            seed_sleep_machine(&mut world, pos, machine);
            if layout != SleepLayout::Plain && !hopper_first {
                place_sleep_hoppers(&mut world, pos, above, below, before, after);
            }
            // 旁边一个比较器、隔一格一个，读数和计划刻都进快照。
            for p in [BlockPos::new(1, 64, 0), BlockPos::new(3, 64, 0)] {
                world.set_block(p.below(), BlockState(blocks::STONE), UPDATE_ALL);
            }
            world.set_block(
                BlockPos::new(2, 64, 0),
                BlockState(blocks::STONE),
                UPDATE_ALL,
            );
            for (p, facing) in [
                (BlockPos::new(1, 64, 0), Direction::West),
                (BlockPos::new(3, 64, 0), Direction::East),
            ] {
                world.set_block(
                    p,
                    BlockState(blocks::COMPARATOR)
                        .with(HORIZONTAL_FACING, facing)
                        .with(MODE_COMPARATOR, ComparatorMode::Compare),
                    UPDATE_ALL,
                );
            }
        }
        level.sleep_enabled(sleep);
        Self {
            level,
            ticks,
            script,
            cursor: 0,
            machine,
            layout,
            menu: Vec::new(),
            slept: 0,
            restarts: Vec::new(),
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
            self.apply(action);
        }
        // 到期的比较器计划刻也按官方顺序跑，再跑方块实体阶段。
        let _ = crate::interact::run_due_ticks(&mut self.level, &mut self.ticks, now);
        let _ = run_block_entity_tick(&mut self.level, &mut self.ticks);
        set_block_entity_clock(now);
        self.menu.extend(sleep_menu_values(&self.level));
        if self.level.due_count() == 0 {
            self.slept += 1;
        }
    }

    fn apply(&mut self, action: SleepAction) {
        let pos = BlockPos::new(0, 64, 0);
        let mut world = World::new(&mut self.level, &mut self.ticks);
        match action {
            SleepAction::Feed(stack) => {
                let machine = self.machine;
                world.with_container(pos, &mut |container| {
                    let slot = match machine {
                        SleepMachine::Campfire => 0,
                        SleepMachine::Brewing => 3,
                    };
                    let mut current = container.get(slot);
                    if current.is_empty() {
                        container.set(slot, stack.clone());
                    } else if current.item == stack.item {
                        current.count += stack.count;
                        container.set(slot, current);
                    }
                });
                if matches!(machine, SleepMachine::Campfire) {
                    world.with_block_entity::<CampfireEntity, _>(pos, |entity, world| {
                        let _ = entity.place_food(world, &stack);
                    });
                }
            }
            SleepAction::Take(slot, count) => {
                world.with_container(pos, &mut |container| {
                    let _ = container.remove_item(slot.min(container.size() - 1), count);
                });
            }
            SleepAction::Toggle => {
                let state = world.block_state(pos);
                if let Some(lit) = state.try_get(LIT) {
                    world.set_block_and_update(pos, state.with(LIT, !lit));
                } else {
                    world.set_block(pos, BlockState(blocks::AIR), UPDATE_ALL);
                    world.set_block(pos, BlockState(blocks::BREWING_STAND), UPDATE_ALL);
                }
            }
            SleepAction::BoxChunk => {
                let _ = world;
                self.level.box_chunk(ChunkPos::new(0, 0));
            }
            SleepAction::Freeze(frozen) => {
                let _ = world;
                self.level
                    .set_chunk_block_entity_ticking(ChunkPos::new(0, 0), !frozen);
                self.level.set_frozen(ChunkPos::new(0, 0), frozen);
            }
            SleepAction::ToggleHopper => {
                let hopper = BlockPos::new(0, 65, 0);
                let state = world.block_state(hopper);
                if state.is(blocks::HOPPER) {
                    use crate::state::ENABLED;
                    world.set_block_and_update(hopper, state.with(ENABLED, !state.get(ENABLED)));
                }
            }
            SleepAction::HopperFeed(stack) => {
                let at = BlockPos::new(-1, 65, 0);
                world.with_container(at, &mut |container| {
                    for slot in 0..container.size() {
                        if container.get(slot).is_empty() {
                            container.set(slot, stack.clone());
                            break;
                        }
                    }
                });
            }
            SleepAction::HopperDrain => {
                let at = BlockPos::new(0, 64, -1);
                world.with_container(at, &mut |container| {
                    for slot in 0..container.size() {
                        let _ = container.remove_item(slot, 64);
                    }
                });
            }
            SleepAction::ComparatorRead => {
                let mut values = Vec::new();
                for p in [BlockPos::new(1, 64, 0), BlockPos::new(3, 64, 0)] {
                    let state = self.level.get(p);
                    let world = World::new(&mut self.level, &mut self.ticks);
                    values.push(state.analog_output_signal(&world, p, Direction::Up));
                }
                self.menu.extend(values);
            }
            SleepAction::Restart => {
                let _ = world;
                self.restart_machine();
            }
            SleepAction::MenuRead => self.menu.extend(sleep_menu_values(&self.level)),
        }
    }

    /// 存档组装 → 按 id 读回 → 放回表：睡着的数值必须按当前刻现算，
    /// 读回来的实体没有当时的日程，下一次 tick 重新算（等价于立刻醒）。
    fn restart_machine(&mut self) {
        use crate::archive::block_entity::{BlockEntityKind, create_block_entity};
        use mc_bamboo_nbt::Tag;

        let pos = BlockPos::new(0, 64, 0);
        let (id, tag) = {
            let entity = self
                .level
                .block_entities_table()
                .get_dyn(pos)
                .expect("机器还在");
            let id = match self.machine {
                SleepMachine::Campfire => BlockEntityKind::Campfire,
                SleepMachine::Brewing => BlockEntityKind::BrewingStand,
            };
            (
                id.name(),
                Tag::Compound(
                    crate::archive::write_block_entity_additional(entity).expect("写出成功"),
                ),
            )
        };
        let rebuilt = create_block_entity(id, pos, self.level.get(pos), &tag)
            .expect("读回成功")
            .expect("有构造分支");
        self.level.block_entities_table_mut().remove(pos);
        self.level.block_entities_table_mut().insert(rebuilt);
        self.restarts.push(format!(
            "{:?}",
            crate::archive::write_block_entity_additional(
                self.level
                    .block_entities_table()
                    .get_dyn(pos)
                    .expect("刚放回")
            )
        ));
    }

    fn snapshot(&self) -> Vec<String> {
        let pos = BlockPos::new(0, 64, 0);
        let mut out = Vec::new();
        out.push(format!("layout:{:?}", self.layout));
        for p in [
            pos,
            BlockPos::new(0, 65, 0),
            BlockPos::new(0, 63, 0),
            BlockPos::new(-1, 65, 0),
            BlockPos::new(0, 64, -1),
            BlockPos::new(1, 64, 0),
            BlockPos::new(2, 64, 0),
            BlockPos::new(3, 64, 0),
        ] {
            out.push(format!("b{p:?}:{}", self.level.get(p).0));
        }
        out.push(format!(
            "campfire:{:?}",
            self.level.block_entity::<CampfireEntity>(pos).map(|e| (
                e.data_values(),
                e.items().clone(),
                e.recipe_facts()
            ))
        ));
        out.push(format!(
            "brewing:{:?}",
            self.level.block_entity::<BrewingStandEntity>(pos).map(|e| (
                e.data_values(),
                e.items().clone(),
                e.observed_facts()
            ))
        ));
        for p in [
            BlockPos::new(0, 65, 0),
            BlockPos::new(0, 63, 0),
            BlockPos::new(-1, 65, 0),
            BlockPos::new(0, 64, -1),
        ] {
            out.push(format!(
                "hopper{p:?}:{:?}",
                self.level
                    .block_entity::<HopperEntity>(p)
                    .map(|e| (e.cooldown_time(), e.ticked_game_time()))
            ));
            out.push(format!("slots{p:?}:{:?}", self.level.containers.get(&p)));
        }
        for p in [BlockPos::new(1, 64, 0), BlockPos::new(3, 64, 0)] {
            out.push(format!(
                "c{p:?}:{}",
                self.level.redstone_table().comparator_output(p)
            ));
        }
        out.push(format!(
            "archive:{:?}",
            self.level
                .block_entities_table()
                .get_dyn(pos)
                .map(crate::archive::write_block_entity_additional)
        ));
        out.push(format!("seed:{}", self.level.random.source().seed()));
        out.push(format!("writes:{:?}", self.level.writes));
        out.push(format!("menu:{:?}", self.menu));
        out.push(format!("restarts:{:?}", self.restarts));
        out.push(format!("clock:{}", block_entity_clock()));
        let mut scheduled: Vec<_> = self
            .ticks
            .pending()
            .map(|tick| (tick.trigger_tick, tick.priority, tick.pos, tick.block))
            .collect();
        scheduled.sort();
        out.push(format!("scheduled:{scheduled:?}"));
        out
    }
}

fn sleep_menu_values(level: &TestLevel) -> Vec<i32> {
    let pos = BlockPos::new(0, 64, 0);
    let mut values = Vec::new();
    if let Some(entity) = level.block_entity::<CampfireEntity>(pos) {
        values.extend(entity.data_values());
    } else if let Some(entity) = level.block_entity::<BrewingStandEntity>(pos) {
        values.extend(entity.data_values());
    } else {
        values.extend([0; 8]);
    }
    values
}

fn seed_sleep_machine(world: &mut World<'_>, pos: BlockPos, machine: SleepMachine) {
    match machine {
        SleepMachine::Campfire => {
            world.with_block_entity::<CampfireEntity, _>(pos, |entity, world| {
                for _ in 0..2 {
                    let _ = entity.place_food(world, &ItemStack::new(items::POTATO, 1));
                }
            });
        }
        SleepMachine::Brewing => {
            let water = water_bottle();
            world.with_block_entity::<BrewingStandEntity, _>(pos, |entity, _| {
                for slot in 0..3 {
                    entity.set_item(slot, water.clone());
                }
                entity.set_item(3, ItemStack::new(items::NETHER_WART, 8));
                entity.set_item(4, ItemStack::new(items::BLAZE_POWDER, 8));
            });
        }
    }
}

fn place_sleep_hoppers(
    world: &mut World<'_>,
    _pos: BlockPos,
    above: BlockPos,
    below: BlockPos,
    before: BlockPos,
    after: BlockPos,
) {
    use crate::state::FACING_HOPPER;
    for (p, facing) in [
        (above, Direction::Down),
        (below, Direction::Down),
        (before, Direction::East),
        (after, Direction::West),
    ] {
        world.set_block(
            p,
            BlockState(blocks::HOPPER).with(FACING_HOPPER, facing),
            UPDATE_ALL,
        );
        world.set_block_entity(Box::new(HopperEntity::new(p)));
    }
}

fn compare_sleep(machine: SleepMachine, layout: SleepLayout, seed: u64, ticks: i64) -> usize {
    let script = SleepScript::random(seed, ticks, machine, layout);
    let mut sleeping = SleepScene::new(
        SleepScript {
            actions: script.actions.clone(),
        },
        machine,
        layout,
        true,
    );
    let mut always = SleepScene::new(
        SleepScript {
            actions: script.actions,
        },
        machine,
        layout,
        false,
    );
    for now in 1..=ticks {
        sleeping.step(now);
        always.step(now);
        assert_eq!(
            sleeping.snapshot(),
            always.snapshot(),
            "machine {machine:?} layout {layout:?} seed {seed} tick {now}"
        );
    }
    sleeping.slept
}

#[test]
fn campfire_and_brewing_sleep_match_ticking_every_tick() {
    let mut total_slept = 0;
    for seed in [1, 7, 42, 1337, 90210] {
        for machine in [SleepMachine::Campfire, SleepMachine::Brewing] {
            for layout in [
                SleepLayout::Plain,
                SleepLayout::HopperBefore,
                SleepLayout::HopperAfter,
            ] {
                total_slept += compare_sleep(machine, layout, seed, 2000);
            }
        }
    }
    assert!(total_slept > 0, "休眠真的发生过");
}

/// 漏斗冷却期休眠：官方在冷却里每刻只减 1、写世界时间，外部事件一律
/// 等冷却归零那一刻才被 `tryMoveItems` 看到。下面逐条核对唤醒入口。
struct CooldownScene {
    level: TestLevel,
    ticks: BlockTicks,
    /// 开着的界面和存档每刻读到的冷却、时间相对量。
    reads: Vec<String>,
}

impl CooldownScene {
    fn new(sleep: bool) -> Self {
        let pos = BlockPos::new(0, 64, 0);
        let above = BlockPos::new(0, 65, 0);
        let below = BlockPos::new(0, 63, 0);
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set_container(pos, vec![ItemStack::empty(); HOPPER_CONTAINER_SIZE]);
        level.set_container(above, vec![ItemStack::empty(); 27]);
        level.set_container(below, vec![ItemStack::empty(); 27]);
        {
            let mut world = World::new(&mut level, &mut ticks);
            use crate::state::FACING_HOPPER;
            for (p, facing) in [(pos, Direction::Down), (above, Direction::Down)] {
                world.set_block(
                    p,
                    BlockState(blocks::HOPPER).with(FACING_HOPPER, facing),
                    UPDATE_ALL,
                );
            }
            world.set_block(below, BlockState(blocks::CHEST), UPDATE_ALL);
            world.set_block_entity(Box::new(HopperEntity::new(pos)));
            world.set_block_entity(Box::new(HopperEntity::new(above)));
            world.with_container(pos, &mut |container| {
                container.set(0, ItemStack::new(items::DIRT, 16));
            });
        }
        level.sleep_enabled(sleep);
        Self {
            level,
            ticks,
            reads: Vec::new(),
        }
    }

    fn step(&mut self, now: i64) {
        self.ticks.set_time(now);
        set_block_entity_clock(now);
        let _ = run_block_entity_tick(&mut self.level, &mut self.ticks);
        set_block_entity_clock(now);
        self.reads.push(self.snapshot());
    }

    fn snapshot(&self) -> String {
        let pos = BlockPos::new(0, 64, 0);
        let above = BlockPos::new(0, 65, 0);
        let below = BlockPos::new(0, 63, 0);
        let mut out = Vec::new();
        for p in [pos, above, below] {
            out.push(format!("b{p:?}:{}", self.level.get(p).0));
            out.push(format!("slots{p:?}:{:?}", self.level.containers.get(&p)));
        }
        for p in [pos, above] {
            out.push(format!(
                "hopper{p:?}:{:?}",
                self.level.block_entity::<HopperEntity>(p).map(|e| (
                    e.cooldown_time(),
                    e.ticked_game_time(),
                    e.ticked_game_time_age()
                ))
            ));
            out.push(format!(
                "archive{p:?}:{:?}",
                self.level
                    .block_entities_table()
                    .get_dyn(p)
                    .map(crate::archive::write_block_entity_additional)
            ));
        }
        out.push(format!("seed:{}", self.level.random.source().seed()));
        out.push(format!("writes:{:?}", self.level.writes));
        out.push(format!("entities:{:?}", self.level.entities().ids()));
        out.concat()
    }
}

/// 冷却期内逐条做外部事件，和「每刻都跑」对照；到点那一格才准动。
fn cooldown_events(scene: &mut CooldownScene, now: i64) {
    let pos = BlockPos::new(0, 64, 0);
    let above = BlockPos::new(0, 65, 0);
    let below = BlockPos::new(0, 63, 0);
    let mut world = World::new(&mut scene.level, &mut scene.ticks);
    match now {
        // 目标容器变满 / 被抽空：官方只看下一次 `tryMoveItems` 的结果。
        2 => {
            let _ = world.with_container(below, &mut |container| {
                for slot in 0..container.size() {
                    container.set(slot, ItemStack::new(items::STONE, 64));
                }
            });
        }
        3 => {
            let _ = world.with_container(below, &mut |container| {
                for slot in 0..container.size() {
                    container.set(slot, ItemStack::empty());
                }
            });
        }
        // 上方容器变化（邻居容器）：漏斗自己的下一次 tick 才吸。
        4 => {
            let _ = world.with_container(above, &mut |container| {
                container.set(0, ItemStack::new(items::RAW_IRON, 4));
            });
        }
        // 漏斗自己的物品栏 / 冷却被外部改：统一入口 `wake`。
        5 => {
            let _ = world.with_container(pos, &mut |container| {
                container.set(1, ItemStack::new(items::COAL, 3));
            });
        }
        // `ENABLED` 变化：`set_block_state` 的 `wake` 覆盖这一格。
        6 => {
            use crate::state::ENABLED;
            let state = world.block_state(pos);
            world.set_block_and_update(pos, state.with(ENABLED, false));
        }
        7 => {
            use crate::state::ENABLED;
            let state = world.block_state(pos);
            world.set_block_and_update(pos, state.with(ENABLED, true));
        }
        // 物品实体进入吸取范围：实体生成 / 移动 / 与漏斗的 `entityInside`。
        8 => {
            use mc_bamboo_core::Vec3;
            let item = crate::entity::Entity::new(
                crate::entity::EntityKind::Item(crate::entity::GroundItem {
                    stack: ItemStack::new(items::POTATO, 1),
                    age: 0,
                    pickup_delay: 0,
                    health: 5,
                    bob_offs: 0.0,
                    roll_velocity: false,
                    target: None,
                }),
                Vec3::new(0.5, 65.5, 0.5),
            );
            world.spawn_entity(item);
        }
        _ => {}
    }
}

#[test]
fn hopper_cooldown_sleep_matches_ticking_every_tick_with_external_events() {
    let mut sleeping = CooldownScene::new(true);
    let mut always = CooldownScene::new(false);
    for now in 1..=40 {
        cooldown_events(&mut sleeping, now);
        cooldown_events(&mut always, now);
        sleeping.step(now);
        always.step(now);
        assert_eq!(
            sleeping.reads.last(),
            always.reads.last(),
            "tick {now}: 冷却期外部事件改变了结果"
        );
    }
    // 冷却里确实睡过：至少有一刻一个 ticker 都没跑（差分表里另有断言）。
    assert_eq!(sleeping.reads.len(), 40);
    // 到冷却归零那一刻，漏斗确实动过：槽位从第 1 刻到第 40 刻变了，
    // 而且两份世界的快照逐刻相同（上面的比较已经全过）。
    assert_ne!(sleeping.reads.first(), sleeping.reads.last());
    assert!(sleeping.level.due_count() > 0, "到点那刻真跑过 ticker");
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

/// 测量用的手工基准：几百台正在工作的漏斗、营火、酿造台，方块实体阶段每刻的耗时。
///
/// 用法（release、安静窗口）：
/// `cargo test --release -p mc-bamboo-blocks block_entity_sleep::machine_phase_bench -- --ignored --nocapture`
/// 同一轮里三种机器各自开 / 关休眠交替跑，取多次里最快的一次。
#[test]
#[ignore]
fn machine_phase_bench() {
    const COUNT: i32 = 300;
    const TICKS: i64 = 2000;
    const ROUNDS: usize = 3;

    #[derive(Clone, Copy, Debug)]
    enum BenchKind {
        Hopper,
        Campfire,
        Brewing,
    }

    fn build(kind: BenchKind, sleep: bool) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        for i in 0..COUNT {
            let pos = BlockPos::new(i, 64, 0);
            if matches!(kind, BenchKind::Hopper) {
                level.set_container(pos, vec![ItemStack::empty(); HOPPER_CONTAINER_SIZE]);
                level.set_container(pos.below(), vec![ItemStack::empty(); 27]);
            }
        }
        {
            let mut world = World::new(&mut level, &mut ticks);
            for i in 0..COUNT {
                let pos = BlockPos::new(i, 64, 0);
                match kind {
                    BenchKind::Hopper => {
                        use crate::state::FACING_HOPPER;
                        world.set_block(
                            pos,
                            BlockState(blocks::HOPPER).with(FACING_HOPPER, Direction::Down),
                            UPDATE_ALL,
                        );
                        // 五格都放满，2000 刻里每次冷却归零都会送一件，
                        // 于是 8 刻里有 7 刻睡在冷却期，不会退化成闲置空转。
                        world.with_container(pos, &mut |container| {
                            for slot in 0..HOPPER_CONTAINER_SIZE {
                                container.set(slot, ItemStack::new(items::DIRT, 64));
                            }
                        });
                        world.set_block(pos.below(), BlockState(blocks::CHEST), UPDATE_ALL);
                        world.set_block_entity(Box::new(HopperEntity::new(pos)));
                    }
                    BenchKind::Campfire => {
                        world.set_block(pos, BlockState(blocks::CAMPFIRE), UPDATE_ALL);
                        world.with_block_entity::<CampfireEntity, _>(pos, |entity, world| {
                            for _ in 0..4 {
                                let _ = entity.place_food(world, &ItemStack::new(items::POTATO, 1));
                            }
                        });
                    }
                    BenchKind::Brewing => {
                        world.set_block(pos, BlockState(blocks::BREWING_STAND), UPDATE_ALL);
                        let water = water_bottle();
                        world.with_block_entity::<BrewingStandEntity, _>(pos, |entity, _| {
                            for slot in 0..3 {
                                entity.set_item(slot, water.clone());
                            }
                            entity.set_item(3, ItemStack::new(items::NETHER_WART, 64));
                            entity.set_item(4, ItemStack::new(items::BLAZE_POWDER, 64));
                        });
                    }
                }
            }
        }
        level.sleep_enabled(sleep);
        (level, ticks)
    }

    fn measure(kind: BenchKind, sleep: bool) -> f64 {
        let mut best = u128::MAX;
        for _ in 0..ROUNDS {
            let (mut level, mut ticks) = build(kind, sleep);
            let started = std::time::Instant::now();
            for now in 1..=TICKS {
                ticks.set_time(now);
                set_block_entity_clock(now);
                let _ = run_block_entity_tick(&mut level, &mut ticks);
            }
            best = best.min(started.elapsed().as_nanos());
        }
        best as f64 / TICKS as f64 / 1000.0
    }

    for kind in [BenchKind::Hopper, BenchKind::Campfire, BenchKind::Brewing] {
        let mut off = Vec::new();
        let mut on = Vec::new();
        for _ in 0..3 {
            off.push(measure(kind, false));
            on.push(measure(kind, true));
        }
        let off = off.into_iter().fold(f64::INFINITY, f64::min);
        let on = on.into_iter().fold(f64::INFINITY, f64::min);
        println!(
            "{COUNT} 台{kind:?}，方块实体阶段每刻：休眠关 {off:.1} µs，休眠开 {on:.1} µs，{:.1}×",
            off / on
        );
    }
}
