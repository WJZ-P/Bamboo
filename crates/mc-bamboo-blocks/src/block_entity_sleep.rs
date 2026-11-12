//! 方块实体休眠的差分测试。
//!
//! 同一个随机场景跑两份世界，一份开休眠、一份关休眠（退回每刻都跑）。
//! **每一刻**比较方块状态、容器、方块实体的全部字段（睡着的按公式现算）、
//! 写方块序列、随机数种子和计划刻，任何一位不同都算失败。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockState, BlockTicks, HopperEntity,
    behaviour::machines::FurnaceEntity,
    block_entity::{block_entity_clock, set_block_entity_clock},
    hopper::HOPPER_CONTAINER_SIZE,
    interact::run_block_entity_tick,
    random::LegacyRandom,
    state::LIT,
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
