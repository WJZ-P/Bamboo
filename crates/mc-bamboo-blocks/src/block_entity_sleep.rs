//! 方块实体休眠的差分测试：同一场景开 / 关休眠，逐刻比较全部可见状态。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockState, BlockTicks,
    behaviour::machines::FurnaceEntity,
    block_entity::set_block_entity_clock,
    interact::run_block_entity_tick,
    testing::TestLevel,
    world::{UPDATE_ALL, World},
};

const POS: BlockPos = BlockPos::new(0, 64, 0);

struct Scene {
    level: TestLevel,
    ticks: BlockTicks,
    /// 上一刻有多少个 ticker 真跑过。
    ran: usize,
}

impl Scene {
    fn new(sleep: bool) -> Self {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block(POS, BlockState(blocks::FURNACE), UPDATE_ALL);
            world.with_block_entity::<FurnaceEntity, _>(POS, |entity, _| {
                entity.write_item(0, ItemStack::new(items::RAW_IRON, 8));
                entity.write_item(1, ItemStack::new(items::COAL, 4));
            });
        }
        level.sleep_enabled(sleep);
        Self {
            level,
            ticks,
            ran: 0,
        }
    }

    fn step(&mut self, now: i64) {
        self.ticks.set_time(now);
        set_block_entity_clock(now);
        let _ = run_block_entity_tick(&mut self.level, &mut self.ticks);
        // `end_tick` 会清掉本刻的 due，所以在驱动返回前记录的计数取不到。
        // 用 `begin_tick` 之后留下的 awake 集合大小代替：睡着的条目不在里面。
        self.ran = self.level.due_count();
    }

    fn snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("state:{:?}", self.level.get(POS).0));
        let furnace = self.level.block_entity::<FurnaceEntity>(POS).unwrap();
        out.push(format!("data:{:?}", furnace.data_values()));
        out.push(format!("items:{:?}", furnace.items()));
        out.push(format!("facts:{:?}", furnace.recipe_facts()));
        out.push(format!("speed:{}", furnace.speed_multiplier().to_bits()));
        out.push(format!("seed:{}", self.level.random.source().seed()));
        out.push(format!("writes:{:?}", self.level.writes));
        out
    }
}

#[test]
fn furnace_sleep_matches_ticking_every_tick() {
    let mut sleeping = Scene::new(true);
    let mut always = Scene::new(false);
    let mut slept = 0;
    for now in 1..=3000 {
        sleeping.step(now);
        always.step(now);
        assert_eq!(sleeping.snapshot(), always.snapshot(), "tick {now}");
        if sleeping.level.due_count() == 0 {
            slept += 1;
        } else if now < 30 || now % 200 < 3 {
            println!("ran at {now}: {:?}", sleeping.snapshot());
        }
    }
    // 大部分刻应该完全不跑熔炉；否则休眠没有真正生效。
    assert!(slept > 2000, "only {slept} of 3000 ticks slept");
}
