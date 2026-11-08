//! TNT 的爆炸，官方 `ServerExplosion` 里和方块有关的那部分。
//!
//! 只走 TNT 这一条路：不伤生物、不点火。被炸掉的方块按战利品表掉物品，不给经验。
//! 破坏哪些方块、按什么顺序破坏都要和官方一样，因为顺序决定世界随机源怎么用，
//! 进而决定连锁点燃的 TNT 拿到什么引信。
//!
//! The block half of `ServerExplosion`, TNT only. Mobs are not hurt and no fire is
//! started. Broken blocks drop through their loot tables, without experience.
//! Which blocks break, and in which order, matches vanilla because that order decides
//! how `level.random` is consumed and therefore which fuse a chained TNT gets.

use std::collections::HashSet;

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::BlockAncestor;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    state::{BlockState, FluidState},
    world::World,
};

/// `ServerExplosion.calculateExplodedPositions` 的网格边长，只有最外面一层格子发出射线。
const RAY_GRID: i32 = 16;
/// 射线每走一格消耗的爆炸强度，官方写的是 `0.22500001F`。
const POWER_PER_STEP: f32 = 0.225_000_01;
/// 射线每一步走多远，官方的 `0.3F`。
const STEP: f32 = 0.3;
/// 方块抗性之外再加的抗性，官方的 `0.3F`。
const RESISTANCE_BONUS: f32 = 0.3;
/// 每点抗性消耗多少爆炸强度，官方的 `0.3F`。
const RESISTANCE_COST: f32 = 0.3;
impl World<'_> {
    /// `PrimedTnt.explode` 走到 `ServerLevel.explode` 和 `ServerExplosion.explode`，
    /// 这里只保留破坏方块和连锁点燃 TNT。
    ///
    /// `center` 是爆炸中心，`radius` 是爆炸强度。不伤生物，也不给经验。
    ///
    /// `center` is the explosion centre and `radius` is its power. Mobs are not hurt and no
    /// experience is dropped.
    pub fn explode_tnt(&mut self, center: Vec3, radius: f32) {
        let mut broken = self.calculate_exploded_positions(center, radius);
        // `hurtEntities` 在打乱方块之前，而且不取 `level.random`。
        // `hurtEntities` runs before the block list is shuffled and does not draw `level.random`.
        crate::entity::item::hurt_items_by_explosion(self, center, radius);
        // `Util.shuffle(toBlow, level.random)`。
        self.random().shuffle(&mut broken);
        // `interactWithBlocks`：先收齐掉落，整圈方块处理完才 `popResource`。
        // `interactWithBlocks` collects drops, then `popResource`s them after every block.
        let mut drops = Vec::new();
        for pos in broken {
            let state = self.block_state(pos);
            if state.is_air() {
                continue;
            }
            // `TntBlock.dropFromExplosion` 返回假，别的方块默认掉落。
            // `TntBlock.dropFromExplosion` is false. Every other block drops by default.
            let was_tnt = state.is_a(BlockAncestor::TntBlock);
            if !was_tnt {
                for stack in self.explosion_block_drops(state, pos) {
                    collect_drop(&mut drops, pos, stack);
                }
            }
            // `setBlockAndUpdate` 在 `wasExploded` 之前。容器的 `dropContents` 就在这一步取数。
            // `setBlockAndUpdate` runs before `wasExploded`. Container `dropContents` draws here.
            self.set_block_and_update(pos, BlockState::AIR);
            if was_tnt {
                self.spawn_short_fuse_tnt(pos);
            }
        }
        for (pos, stack) in drops {
            self.pop_resource(pos, stack);
        }
    }

    /// `ServerExplosion.calculateExplodedPositions`：从 16 立方网格最外层每一格朝外射一条线，
    /// 线上剩多少爆炸强度就炸掉沿途的方块。
    ///
    /// 每条线开头从世界随机源取一个数，所以这个函数的调用次数和顺序都影响后续的随机数。
    fn calculate_exploded_positions(&mut self, center: Vec3, radius: f32) -> Vec<BlockPos> {
        let mut blown = PositionSet::default();
        for xx in 0..RAY_GRID {
            for yy in 0..RAY_GRID {
                for zz in 0..RAY_GRID {
                    if xx != 0
                        && xx != RAY_GRID - 1
                        && yy != 0
                        && yy != RAY_GRID - 1
                        && zz != 0
                        && zz != RAY_GRID - 1
                    {
                        continue;
                    }
                    // 官方的坐标和长度都是 `float` 算完才变成 `double`。
                    let xd = f64::from(xx as f32 / 15.0 * 2.0 - 1.0);
                    let yd = f64::from(yy as f32 / 15.0 * 2.0 - 1.0);
                    let zd = f64::from(zz as f32 / 15.0 * 2.0 - 1.0);
                    let length = (xd * xd + yd * yd + zd * zd).sqrt();
                    let (xd, yd, zd) = (xd / length, yd / length, zd / length);
                    let mut remaining = radius * (0.7 + self.random().next_float() * 0.6);
                    let (mut xp, mut yp, mut zp) = (center.x, center.y, center.z);
                    while remaining > 0.0 {
                        let pos = BlockPos::new(mth::floor(xp), mth::floor(yp), mth::floor(zp));
                        let state = self.block_state(pos);
                        let fluid = state.fluid();
                        if !self.is_in_world_bounds(pos) {
                            break;
                        }
                        if !(state.is_air() && fluid.is_empty()) {
                            let resistance = state.block().explosion_resistance;
                            let resistance = resistance.max(fluid_resistance(fluid));
                            remaining -= (resistance + RESISTANCE_BONUS) * RESISTANCE_COST;
                        }
                        // `ExplosionDamageCalculator.shouldBlockExplode` 恒为真。
                        if remaining > 0.0 {
                            blown.insert(pos);
                        }
                        let step = f64::from(STEP);
                        xp += xd * step;
                        yp += yd * step;
                        zp += zd * step;
                        remaining -= POWER_PER_STEP;
                    }
                }
            }
        }
        blown.into_java_order()
    }
}

/// `ServerExplosion.addOrAppendStack`：先按 16 的上限并进已有的堆，剩下的另起一堆。
///
/// `ServerExplosion.addOrAppendStack`: merge into an existing stack with a cap of 16, then keep
/// whatever is left as a new stack.
fn collect_drop(stacks: &mut Vec<(BlockPos, ItemStack)>, pos: BlockPos, mut stack: ItemStack) {
    for (_, existing) in stacks.iter_mut() {
        crate::entity::merge_explosion(existing, &mut stack);
        if stack.is_empty() {
            return;
        }
    }
    stacks.push((pos, stack));
}

/// `FluidState.getExplosionResistance()`。数值来自生成的流体表。
///
/// `FluidState.getExplosionResistance()`, from the generated fluid table.
fn fluid_resistance(fluid: FluidState) -> f32 {
    fluid.explosion_resistance()
}

/// 爆炸破坏的方块表，官方的 `HashSet<BlockPos>`。
///
/// 遍历顺序是官方 `HashMap` 的桶顺序：桶数从 16 开始翻倍到装得下为止，按桶下标升序，
/// 同一个桶里按加入顺序。桶里的元素多到 8 个又会树化，那时顺序就不一样了；TNT 的半径
/// 只有 4，一组爆炸远到不了那一步。
#[derive(Debug, Default)]
struct PositionSet {
    positions: Vec<BlockPos>,
    seen: HashSet<BlockPos>,
}

impl PositionSet {
    /// `HashSet.add`。
    fn insert(&mut self, pos: BlockPos) {
        if self.seen.insert(pos) {
            self.positions.push(pos);
        }
    }

    /// `new ObjectArrayList(set)`：按哈希表的遍历顺序排好。
    fn into_java_order(self) -> Vec<BlockPos> {
        let capacity = table_capacity(self.positions.len());
        let mut buckets = vec![Vec::new(); capacity];
        for pos in self.positions {
            buckets[block_pos_bucket(pos, capacity)].push(pos);
        }
        buckets.into_iter().flatten().collect()
    }
}

/// `HashMap` 装下这么多个元素后的桶数。
fn table_capacity(len: usize) -> usize {
    let mut capacity = 16;
    while len > capacity / 4 * 3 {
        capacity *= 2;
    }
    capacity
}

/// `BlockPos` 落在哪个桶里：`Vec3i.hashCode()`，再按 `HashMap.hash` 和桶数取下标。
fn block_pos_bucket(pos: BlockPos, capacity: usize) -> usize {
    let hash = pos
        .y
        .wrapping_add(pos.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x);
    let spread = hash ^ ((hash as u32) >> 16) as i32;
    (spread as u32 & (capacity as u32 - 1)) as usize
}
