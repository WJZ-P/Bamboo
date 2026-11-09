//! 生存和冒险模式挖掉方块之后的 `Block.playerDestroy` → `dropResources`。
//!
//! 战利品走表上的 `random_sequence`，不碰 `level.random`。每个掉落堆再按
//! `Block.popResource` 从 `level.random` 取三个位置 double。
//!
//! Survival and adventure `Block.playerDestroy` → `dropResources`.
//! Loot uses the table's `random_sequence` and does not touch `level.random`.
//! Each dropped stack then draws three position doubles from `level.random`.

use mc_bamboo_blocks::{BlockState, World};
use mc_bamboo_core::BlockPos;
use mc_bamboo_loot::{BlockDropParams, LevelView, LootStack, block_drops};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_world::store::ChunkStore;
use tracing::debug;

use crate::{inbound::PlayerId, loot_stacks, region::Region};

/// 挖掉 `state` 之后生成掉落。工具上的附魔组件还没转成战利品能读的 JSON。
///
/// Spawns the drops of a broken `state`. Enchantments on the tool are not yet
/// converted into the JSON the loot tables read.
pub(crate) fn drop_broken_block(
    region: &mut Region,
    state: BlockState,
    pos: BlockPos,
    player: PlayerId,
) {
    let tool = region.players.get(&player).and_then(|player| {
        let held = player.inventory.held(mc_bamboo_core::Hand::Main);
        if held.is_empty() {
            None
        } else {
            Some(LootStack::new(held.item, held.count))
        }
    });
    let view = ChunkLevel {
        chunks: &region.chunks,
    };
    let params = BlockDropParams {
        state,
        pos: (pos.x, pos.y, pos.z),
        level: &view,
        block_entity: None,
        breaker: None,
        tool,
        explosion_radius: None,
        luck: 0.0,
    };
    let stacks = match block_drops(
        &params,
        Some(&region.loot_sequences),
        region.random.source(),
    ) {
        Ok(stacks) => stacks,
        Err(error) => {
            debug!(%error, "skipping block drops: the loot table needs data this world does not have");
            return;
        }
    };
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    for stack in stacks {
        pop_loot_stack(&mut world, pos, stack);
    }
}

/// 爆炸里 `state.getDrops`。工具是空的，没有爆炸半径。
///
/// `state.getDrops` during an explosion. The tool is empty and there is no explosion radius.
pub(crate) fn explosion_loot(
    chunks: &ChunkStore,
    sequences: &mc_bamboo_loot::RandomSequences,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
    state: BlockState,
    pos: BlockPos,
) -> Vec<ItemStack> {
    let view = ChunkLevel { chunks };
    let params = BlockDropParams {
        state,
        pos: (pos.x, pos.y, pos.z),
        level: &view,
        block_entity: None,
        breaker: None,
        tool: None,
        explosion_radius: None,
        luck: 0.0,
    };
    match block_drops(&params, Some(sequences), fallback) {
        Ok(stacks) => stacks.iter().map(loot_stacks::loot_stack).collect(),
        Err(error) => {
            debug!(%error, "skipping explosion drops: the loot table needs data this world does not have");
            Vec::new()
        }
    }
}

fn pop_loot_stack(world: &mut World, pos: BlockPos, stack: LootStack) {
    // 空堆也要 `popResource`，三个位置 double 照取。
    // An empty stack still runs `popResource`, so the three position doubles are drawn.
    world.pop_resource(pos, loot_stacks::loot_stack(&stack));
}

/// 战利品条件要问的世界。群系、天气和亮度没有，问到就让求值报不支持。
///
/// The world loot conditions can ask. Biome, weather, and light are absent, so a
/// condition that needs them fails as unsupported.
pub(crate) struct ChunkLevel<'a> {
    pub(crate) chunks: &'a ChunkStore,
}

impl LevelView for ChunkLevel<'_> {
    fn is_loaded(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        Some(self.chunks.block(BlockPos::new(x, y, z)).is_some())
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        self.chunks.block(BlockPos::new(x, y, z)).map(BlockState)
    }

    fn biome(&self, _: i32, _: i32, _: i32) -> Option<i32> {
        None
    }

    fn can_see_sky(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        None
    }

    fn is_raining(&self) -> Option<bool> {
        None
    }

    fn is_thundering(&self) -> Option<bool> {
        None
    }

    fn day_time(&self) -> Option<i64> {
        None
    }

    fn game_time(&self) -> Option<i64> {
        None
    }

    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }
}
