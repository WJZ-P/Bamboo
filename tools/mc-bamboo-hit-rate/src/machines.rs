//! 八台机器的摆法。
//!
//! 方块直接按状态摆进稀疏表，再给需要启动的方块排一条计划刻。周期性输入统一用
//! 「拉杆 + 一格红石线」：实验程序每隔若干刻翻一次拉杆，和玩家拉拉杆走同一套
//! 方块行为。这样时钟不依赖环形电路的自激顺序，也不碰世界时间。

use mc_bamboo_blocks::{
    BlockState, BlockTicks,
    behaviour::machines::FurnaceEntity,
    state::{ComparatorMode, DELAY, FACING, HORIZONTAL_FACING, LIT, MODE_COMPARATOR},
};
use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{CookingKind, blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::world::{BenchLevel, Input, Machine};

const Y: i32 = 64;
fn at(x: i32, z: i32) -> BlockPos {
    BlockPos::new(x, Y, z)
}

fn floor(level: &mut BenchLevel, x0: i32, x1: i32, z0: i32, z1: i32) {
    for x in x0..=x1 {
        for z in z0..=z1 {
            level.set(at(x, z).below(), BlockState(blocks::STONE));
        }
    }
}

fn machine(name: &'static str, input: Input) -> Machine {
    Machine {
        name,
        level: BenchLevel::new(),
        ticks: BlockTicks::new(),
        input,
        cache: None,
        border_period: None,
        effects: Vec::new(),
        sound_draws: 0,
    }
}

/// 给一个方块排一条 `delay` 刻后到期的计划刻。
fn kick(ticks: &mut BlockTicks, pos: BlockPos, state: BlockState, delay: i32) {
    ticks.schedule(
        pos,
        state.block_id(),
        delay,
        mc_bamboo_blocks::TickPriority::Normal,
    );
}

/// 一个周期开关：这一格在红石块（满信号）和空气之间翻转，每 `period` 刻一次。
fn clock_input(m: &mut Machine, pos: BlockPos, period: i64) {
    m.level.set(pos, BlockState::AIR);
    m.input = Input::Clock { pos, period };
}

/// 用官方的 `onPlace` 让每段线按邻居算连接和功率，不手写属性。
fn connect_wires(m: &mut Machine) {
    use mc_bamboo_blocks::World;
    let positions: Vec<BlockPos> = m
        .level
        .blocks
        .iter()
        .filter(|(_, state)| state.is(blocks::REDSTONE_WIRE))
        .map(|(pos, _)| *pos)
        .collect();
    let mut world = World::new(&mut m.level, &mut m.ticks);
    for pos in positions {
        let state = world.block_state(pos);
        state.on_place(&mut world, pos, BlockState::AIR);
    }
}

/// 在 `z=0` 一行摆 `count` 个朝西的中继器，从 `x0` 开始。
///
/// 中继器从它背后（`HORIZONTAL_FACING` 指的那格）读输入、向前（`facing.opposite()`）
/// 输出，所以朝西就是左进右出。
fn repeater_line(m: &mut Machine, x0: i32, count: i32, delay: i32) {
    for x in x0..x0 + count {
        m.level.set(
            at(x, 0),
            BlockState(blocks::REPEATER)
                .with(HORIZONTAL_FACING, Direction::West)
                .with(DELAY, delay),
        );
    }
}

/// 中继器时钟：周期输入驱动四个中继器串成的线，末端一盏红石灯。
pub fn repeater_clock() -> Machine {
    let mut m = machine("中继器时钟", Input::None);
    floor(&mut m.level, -1, 6, -1, 2);
    clock_input(&mut m, at(0, 0), 8);
    for x in 1..=2 {
        m.level.set(at(x, 0), BlockState(blocks::REDSTONE_WIRE));
    }
    repeater_line(&mut m, 3, 4, 2);
    m.level.set(at(7, 0), BlockState(blocks::REDSTONE_LAMP));
    connect_wires(&mut m);
    m
}

/// 侦测器时钟：两个侦测器直接对脸，输出侧一盏灯。
pub fn observer_clock() -> Machine {
    let mut m = machine("侦测器时钟", Input::None);
    floor(&mut m.level, -1, 5, -1, 2);
    // A 朝东看 B，B 朝西看 A；A 的背面接一盏灯。
    let a = at(1, 0);
    let b = at(2, 0);
    m.level.set(
        a,
        BlockState(blocks::OBSERVER).with(FACING, Direction::East),
    );
    m.level.set(
        b,
        BlockState(blocks::OBSERVER).with(FACING, Direction::West),
    );
    m.level.set(at(0, 0), BlockState(blocks::REDSTONE_LAMP));
    // 起振：A 到点翻转 POWERED，B 看到形状变化再排自己的脉冲，往复成时钟。
    kick(&mut m.ticks, a, BlockState(blocks::OBSERVER), 2);
    m
}

/// 2×2 的粘性活塞门：四个活塞分两列朝中间推，周期输入翻转。
pub fn piston_door() -> Machine {
    let mut m = machine("活塞门", Input::None);
    floor(&mut m.level, -1, 9, -1, 5);
    for z in 0..=1 {
        m.level.set(
            at(2, z),
            BlockState(blocks::STICKY_PISTON).with(FACING, Direction::East),
        );
        m.level.set(
            at(5, z),
            BlockState(blocks::STICKY_PISTON).with(FACING, Direction::West),
        );
        m.level.set(at(3, z), BlockState(blocks::STONE));
        m.level.set(at(4, z), BlockState(blocks::STONE));
    }
    clock_input(&mut m, at(8, 0), 40);
    for x in 6..=7 {
        m.level.set(at(x, 0), BlockState(blocks::REDSTONE_WIRE));
    }
    for z in 1..=4 {
        m.level.set(at(7, z), BlockState(blocks::REDSTONE_WIRE));
    }
    for x in 0..=2 {
        m.level.set(at(x, 4), BlockState(blocks::REDSTONE_WIRE));
    }
    connect_wires(&mut m);
    m
}

/// 15×15 的红石线阵列，接一个拉杆周期性翻转的输入，测「纯信号传递」。
pub fn redstone_wire_array() -> Machine {
    let mut m = machine("红石线阵列", Input::None);
    floor(&mut m.level, -2, 16, -2, 16);
    for x in 0..15 {
        for z in 0..15 {
            m.level.set(at(x, z), BlockState(blocks::REDSTONE_WIRE));
        }
    }
    m.level.set(at(15, 7), BlockState(blocks::REDSTONE_LAMP));
    clock_input(&mut m, at(-1, 7), 12);
    // 中继器把输入放大成阵列里的方波，避免整片线只靠一格电源衰减。
    m.level.set(
        at(0, 7),
        BlockState(blocks::REPEATER)
            .with(HORIZONTAL_FACING, Direction::West)
            .with(DELAY, 1),
    );
    connect_wires(&mut m);
    m
}

/// 十几个漏斗首尾相接，源头箱子里有几组物品在流，末端接一个箱子。
pub fn hopper_chain() -> Machine {
    let mut m = machine("漏斗链", Input::None);
    for x in 0..12 {
        m.place_hopper(at(x, 0), Direction::East);
    }
    m.level.set(at(12, 0), BlockState(blocks::CHEST));
    m.level
        .set_container(at(12, 0), vec![ItemStack::empty(); 27]);
    let source = at(0, 0).above();
    m.level.set(source, BlockState(blocks::CHEST));
    let mut slots = vec![ItemStack::empty(); 27];
    slots[0] = ItemStack::new(items::DIRT, 64);
    slots[1] = ItemStack::new(items::STONE, 64);
    slots[2] = ItemStack::new(items::COBBLESTONE, 64);
    m.level.set_container(source, slots);
    m.input = Input::Refill {
        pos: source,
        stack: ItemStack::new(items::DIRT, 64),
        period: 8,
    };
    m
}

/// 标准的漏斗加比较器分类单元，四个并排，物品流过。
pub fn item_sorter() -> Machine {
    let mut m = machine("物品分类机", Input::None);
    for i in 0..4 {
        let x = i * 2;
        let hopper = at(x, 0);
        m.place_hopper(hopper, Direction::North);
        let mut slots = vec![ItemStack::empty(); 5];
        slots[0] = ItemStack::new(items::DIRT, 1);
        m.level.set_container(hopper, slots);
        m.level.set(hopper.below(), BlockState(blocks::CHEST));
        m.level
            .set_container(hopper.below(), vec![ItemStack::empty(); 27]);
        m.level.set(
            at(x, 1),
            BlockState(blocks::COMPARATOR)
                .with(HORIZONTAL_FACING, Direction::North)
                .with(MODE_COMPARATOR, ComparatorMode::Subtract),
        );
        m.level.set(at(x, 2), BlockState(blocks::REDSTONE_WIRE));
    }
    for x in 0..8 {
        m.place_hopper(at(x, -1), Direction::East);
    }
    let source = at(0, -1).above();
    m.level.set(source, BlockState(blocks::CHEST));
    let mut slots = vec![ItemStack::empty(); 27];
    slots[0] = ItemStack::new(items::DIRT, 64);
    m.level.set_container(source, slots);
    connect_wires(&mut m);
    m.input = Input::Refill {
        pos: source,
        stack: ItemStack::new(items::DIRT, 16),
        period: 8,
    };
    m
}

/// 比较器阵列：四组比较器 + 中继器，由一个周期输入驱动，输出接红石灯。
///
/// 这台机器其实是又一个时钟（输入每 10 刻翻转，状态跟着回到同一组值），
/// 不是状态很少重复的计数器，名字按实测行为改。
pub fn comparator_array() -> Machine {
    let mut m = machine("比较器阵列", Input::None);
    floor(&mut m.level, -2, 14, -2, 4);
    clock_input(&mut m, at(-1, 0), 10);
    for x in 0..2 {
        m.level.set(at(x, 0), BlockState(blocks::REDSTONE_WIRE));
    }
    for i in 0..4 {
        let x = 2 + i * 3;
        m.level.set(
            at(x, 0),
            BlockState(blocks::REPEATER)
                .with(HORIZONTAL_FACING, Direction::West)
                .with(DELAY, 1),
        );
        m.level.set(
            at(x + 1, 0),
            BlockState(blocks::COMPARATOR)
                .with(HORIZONTAL_FACING, Direction::West)
                .with(MODE_COMPARATOR, ComparatorMode::Compare),
        );
        m.level.set(at(x + 2, 0), BlockState(blocks::REDSTONE_LAMP));
        m.level.set(
            at(x + 1, 1),
            BlockState(blocks::COMPARATOR)
                .with(HORIZONTAL_FACING, Direction::North)
                .with(MODE_COMPARATOR, ComparatorMode::Subtract),
        );
        m.level.set(at(x + 1, 2), BlockState(blocks::REDSTONE_WIRE));
    }
    connect_wires(&mut m);
    m
}

/// 几个在烧东西的熔炉，带漏斗进出。
pub fn furnace_array() -> Machine {
    let mut m = machine("熔炉阵列", Input::None);
    for i in 0..4 {
        let x = i * 3;
        let furnace = at(x, 0);
        m.level
            .set(furnace, BlockState(blocks::FURNACE).with(LIT, true));
        m.place_hopper(furnace.above(), Direction::Down);
        m.place_hopper(furnace.below(), Direction::Down);
        m.level
            .set(furnace.below().below(), BlockState(blocks::CHEST));
        m.level
            .set_container(furnace.below().below(), vec![ItemStack::empty(); 27]);
        let mut entity = FurnaceEntity::new(furnace, CookingKind::Smelting);
        entity.set_item(0, ItemStack::new(items::RAW_IRON, 32));
        entity.set_item(1, ItemStack::new(items::COAL, 64));
        use mc_bamboo_blocks::World;
        let mut world = World::new(&mut m.level, &mut m.ticks);
        world.set_block_entity(Box::new(entity));
    }
    let source = at(-2, 0);
    m.level.set(source, BlockState(blocks::CHEST));
    let mut slots = vec![ItemStack::empty(); 27];
    slots[0] = ItemStack::new(items::RAW_IRON, 64);
    slots[1] = ItemStack::new(items::COAL, 64);
    m.level.set_container(source, slots);
    m.input = Input::Refill {
        pos: source,
        stack: ItemStack::new(items::RAW_IRON, 16),
        period: 40,
    };
    m
}

/// 只给差分用的机器：不在 [`all`] 里，命中率和基准报告的口径不变。
///
/// 熔炉阵列加上来回渐变的世界边界：边界中心在阵列中间，缩小时依次切过各台熔炉
/// 和漏斗，扩大时再依次放回来。界外的方块实体 ticker 整刻不跑（`LevelChunk.isTicking`）。
pub fn border_furnaces() -> Machine {
    let mut m = furnace_array();
    m.name = "边界切过熔炉";
    m.level.border.set_center(4.5, 0.5);
    m.border_period = Some(80);
    m
}

/// 全部机器，按报告表格的顺序。
/// 三个周期互质的时钟（6 / 10 / 14 刻）汇到同一小片红石线和两个比较器上。
///
/// 每个时钟是一串中继器，输入端各按自己的周期翻转。全局状态的周期是
/// 6 / 10 / 14 的最小公倍数 210 刻，汇合处的局部读集合组合数也多，
/// 用来检验设计稿 9.4「计数器状态很少重复、命中率低」那一行。
pub fn coprime_clocks() -> Machine {
    let mut m = machine("互质时钟汇合", Input::None);
    floor(&mut m.level, -1, 14, -1, 8);
    // 三行各一串 3 个中继器，z=0/2/4，输出在 x=4 汇到一条竖线。
    let mut clocks = Vec::new();
    for (z, period) in [(0_i32, 6_i64), (2, 10), (4, 14)] {
        let source = at(0, z);
        clocks.push((source, period));
        for x in 1..=3 {
            m.level.set(
                at(x, z),
                BlockState(blocks::REPEATER)
                    .with(HORIZONTAL_FACING, Direction::West)
                    .with(DELAY, 1),
            );
        }
        for x in 4..=5 {
            m.level.set(at(x, z), BlockState(blocks::REDSTONE_WIRE));
        }
    }
    m.input = Input::Clocks(clocks);
    // 汇合竖线 z=0..4 在 x=6，再接两个比较器和一盏灯。
    for z in 0..=4 {
        m.level.set(at(6, z), BlockState(blocks::REDSTONE_WIRE));
    }
    m.level.set(
        at(7, 2),
        BlockState(blocks::COMPARATOR)
            .with(HORIZONTAL_FACING, Direction::West)
            .with(MODE_COMPARATOR, ComparatorMode::Compare),
    );
    m.level.set(
        at(8, 2),
        BlockState(blocks::COMPARATOR)
            .with(HORIZONTAL_FACING, Direction::West)
            .with(MODE_COMPARATOR, ComparatorMode::Subtract),
    );
    m.level.set(at(9, 2), BlockState(blocks::REDSTONE_LAMP));
    connect_wires(&mut m);
    m
}

pub fn all() -> Vec<fn() -> Machine> {
    vec![
        repeater_clock,
        observer_clock,
        piston_door,
        redstone_wire_array,
        hopper_chain,
        item_sorter,
        comparator_array,
        coprime_clocks,
        furnace_array,
    ]
}
