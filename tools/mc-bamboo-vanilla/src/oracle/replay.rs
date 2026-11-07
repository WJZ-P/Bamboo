//! 用 `mc-bamboo-blocks` 重放一道题。

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    slice,
};

use mc_bamboo_blocks::{
    Aabb, Actor, BlockHit, BlockState, BlockTicks, Effect, InteractionResult, Level, Outcome,
    TickPriority, destroy_block, run_scheduled_ticks,
    shape::{self, BooleanOp},
    use_item_on,
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;

use super::{
    Names,
    lines::{Action, Case, Cell, Destroyed, Double, Header, Kind},
};

/// `Player.DEFAULT_EYE_HEIGHT`。
const STANDING_EYE_HEIGHT: f32 = 1.62;
/// `Player.CROUCH_DIMENSIONS` 的眼睛高度。
const CROUCHING_EYE_HEIGHT: f32 = 1.27;
/// 创造模式的 `blockInteractionRange`。
const CREATIVE_INTERACTION_RANGE: f64 = 5.0;

/// 这个世界在 Rust 这边的答案。
#[derive(Debug)]
pub struct Replayed {
    pub result: &'static str,
    /// 和场景不同的格子。
    pub changes: BTreeMap<BlockPos, u32>,
    pub destroyed: Vec<Destroyed>,
    /// 这次动作留在队列里的计划刻，按执行顺序。
    pub ticks: Vec<Scheduled>,
    /// 读写过的方块的注册表 ID。结果不对时看它们里面有没有还没移植的类。
    pub touched: BTreeSet<usize>,
}

/// 排着队的计划刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    pub pos: BlockPos,
    pub block: &'static str,
    pub delay: i32,
    pub priority: i32,
}

/// 假世界：方块在表里，世界外是 `void_air`，实体只有场景里的玩家。
struct OracleLevel {
    blocks: HashMap<BlockPos, BlockState>,
    entities: Vec<Aabb>,
    min_y: i32,
    max_y: i32,
    touched: RefCell<BTreeSet<usize>>,
}

impl OracleLevel {
    fn new(case: &Case, header: &Header) -> Self {
        let scene = case.scene.iter();
        Self {
            blocks: scene
                .map(|cell| (cell.pos(), BlockState(cell.state())))
                .collect(),
            entities: entity_boxes(&case.action),
            min_y: header.min_y,
            max_y: header.max_y,
            touched: RefCell::new(BTreeSet::new()),
        }
    }

    fn contains(&self, pos: BlockPos) -> bool {
        (self.min_y..=self.max_y).contains(&pos.y)
    }

    fn touch(&self, state: BlockState) {
        self.touched.borrow_mut().insert(state.block_id());
    }

    /// 和 `scene` 不同的格子，变成空气的格子也算。
    fn changes(&self, scene: &[Cell]) -> BTreeMap<BlockPos, u32> {
        let before: HashMap<BlockPos, u32> = scene
            .iter()
            .map(|cell| (cell.pos(), cell.state()))
            .collect();
        let mut changes: BTreeMap<BlockPos, u32> = self
            .blocks
            .iter()
            .map(|(&pos, state)| (pos, state.0))
            .filter(|&(pos, state)| before.get(&pos).copied().unwrap_or(blocks::AIR) != state)
            .collect();
        changes.retain(|pos, _| self.contains(*pos));
        changes
    }
}

impl Level for OracleLevel {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !self.contains(pos) {
            return BlockState(blocks::VOID_AIR);
        }
        let state = self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR);
        self.touch(state);
        state
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState) -> bool {
        if !self.contains(pos) {
            return false;
        }
        self.touch(state);
        self.blocks.insert(pos, state);
        true
    }

    /// `EntityGetter.isUnobstructed`：先用外框筛一遍实体，再做精确的形状运算。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool {
        let bounds = bounds_of(boxes);
        !self.entities.iter().any(|entity| {
            intersects(bounds, *entity)
                && shape::join_is_not_empty(boxes, slice::from_ref(entity), BooleanOp::And)
        })
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.max_y
    }
}

/// `VoxelShape.bounds`。
fn bounds_of(boxes: &[Aabb]) -> Aabb {
    let fold = |pick: fn(&Aabb) -> f64, merge: fn(f64, f64) -> f64| {
        boxes
            .iter()
            .map(pick)
            .reduce(merge)
            .expect("空形状不用查实体")
    };
    Aabb::new(
        fold(|bounds| bounds.min_x, f64::min),
        fold(|bounds| bounds.min_y, f64::min),
        fold(|bounds| bounds.min_z, f64::min),
        fold(|bounds| bounds.max_x, f64::max),
        fold(|bounds| bounds.max_y, f64::max),
        fold(|bounds| bounds.max_z, f64::max),
    )
}

/// `AABB.intersects`。
fn intersects(first: Aabb, second: Aabb) -> bool {
    first.min_x < second.max_x
        && first.max_x > second.min_x
        && first.min_y < second.max_y
        && first.max_y > second.min_y
        && first.min_z < second.max_z
        && first.max_z > second.min_z
}

fn entity_boxes(action: &Action) -> Vec<Aabb> {
    std::iter::once(&action.actor_box)
        .chain(&action.others)
        .map(aabb)
        .collect()
}

fn aabb(bits: &[Double; 6]) -> Aabb {
    let [min_x, min_y, min_z, max_x, max_y, max_z] = bits.map(|value| value.0);
    Aabb::new(min_x, min_y, min_z, max_x, max_y, max_z)
}

fn actor_of(action: &Action, names: &Names) -> Result<Actor, String> {
    let [x, y, z] = action.actor.map(|value| value.0);
    let eye_height = if action.sneak {
        CROUCHING_EYE_HEIGHT
    } else {
        STANDING_EYE_HEIGHT
    };
    Ok(Actor {
        position: Vec3::new(x, y, z),
        yaw: action.yaw.0,
        // 官方的题里头和身体的朝向相同。
        head_yaw: action.yaw.0,
        pitch: action.pitch.0,
        secondary_use: action.sneak,
        main_hand: names.item(&action.main)?.0,
        off_hand: names.item(&action.off)?.0,
        eye_height: f64::from(eye_height),
        block_interaction_range: CREATIVE_INTERACTION_RANGE,
    })
}

fn hit_of(action: &Action) -> Result<BlockHit, String> {
    let [x, y, z] = action.loc.ok_or("place or use case without a click")?;
    Ok(BlockHit {
        pos: action.target(),
        face: action.face.ok_or("place or use case without a face")?,
        location: Vec3::new(x.0, y.0, z.0),
        inside: action.inside.ok_or("place or use case without inside")?,
    })
}

/// 跑一道题。题目本身有问题时返回错误，`mc-bamboo-blocks` 的 panic 由调用方接住。
pub fn replay(case: &Case, header: &Header, names: &Names) -> Result<Replayed, String> {
    let action = &case.action;
    let mut level = OracleLevel::new(case, header);
    let mut ticks = BlockTicks::new();
    // 队列是空的，这一下只是把时钟拨到官方假世界的时间。
    run_scheduled_ticks(&mut level, &mut ticks, header.game_time);

    let actor = actor_of(action, names)?;
    let outcome = match action.kind {
        Kind::Place | Kind::Use => {
            let hit = hit_of(action)?;
            use_item_on(&mut level, &mut ticks, &actor, action.hand, &hit)
        }
        Kind::Destroy => destroy_block(&mut level, &mut ticks, &actor, action.target()),
        Kind::Tick => run_one_tick(&mut level, &mut ticks, action.target(), header.game_time),
    };

    Ok(Replayed {
        result: result_name(action.kind, outcome.result),
        changes: level.changes(&case.scene),
        destroyed: destroyed_of(&outcome),
        ticks: scheduled_of(&ticks, header.game_time),
        touched: level.touched.into_inner(),
    })
}

/// 官方的题直接调用 `state.tick`，这里排一个立刻到期的刻，走真正的入口。
fn run_one_tick(
    level: &mut OracleLevel,
    ticks: &mut BlockTicks,
    pos: BlockPos,
    now: i64,
) -> Outcome {
    let block = level.block_state(pos).block_id();
    ticks.schedule(pos, block, 0, TickPriority::Normal);
    run_scheduled_ticks(level, ticks, now)
}

fn result_name(kind: Kind, result: InteractionResult) -> &'static str {
    match kind {
        Kind::Tick => "ticked",
        Kind::Destroy if result == InteractionResult::Fail => "refused",
        Kind::Destroy => "destroyed",
        Kind::Place | Kind::Use => match result {
            InteractionResult::Success => "success",
            InteractionResult::SuccessServer => "success_server",
            InteractionResult::Consume => "consume",
            InteractionResult::Fail => "fail",
            InteractionResult::Pass => "pass",
            InteractionResult::TryEmptyHandInteraction => "try_empty_hand",
        },
    }
}

fn destroyed_of(outcome: &Outcome) -> Vec<Destroyed> {
    outcome
        .effects
        .iter()
        .map(|effect| match *effect {
            Effect::Destroyed {
                pos,
                state,
                except_initiator,
            } => Destroyed(pos.x, pos.y, pos.z, state.0, except_initiator),
        })
        .collect()
}

fn scheduled_of(ticks: &BlockTicks, now: i64) -> Vec<Scheduled> {
    ticks
        .pending()
        .map(|tick| Scheduled {
            pos: tick.pos,
            block: blocks::BLOCKS[tick.block].name,
            delay: i32::try_from(tick.trigger_tick - now).expect("计划刻的延迟放得进 i32"),
            priority: tick.priority as i32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    fn level_with_player(player: Aabb) -> OracleLevel {
        OracleLevel {
            blocks: HashMap::new(),
            entities: vec![player],
            min_y: -64,
            max_y: 319,
            touched: RefCell::new(BTreeSet::new()),
        }
    }

    #[test]
    fn outside_the_world_is_void_air_and_cannot_be_written() {
        let mut level = level_with_player(Aabb::new(0.0, 0.0, 0.0, 0.1, 0.1, 0.1));
        let above = BlockPos::new(0, 320, 0);
        assert_eq!(level.block_state(above), BlockState(blocks::VOID_AIR));
        assert!(!level.write_block(above, BlockState(blocks::STONE)));
        assert!(level.touched.borrow().is_empty());
    }

    #[test]
    fn changes_ignore_cells_written_back_to_the_scene() {
        let mut level = level_with_player(Aabb::new(0.0, 0.0, 0.0, 0.1, 0.1, 0.1));
        let kept = BlockPos::new(0, 64, 0);
        let removed = BlockPos::new(0, 65, 0);
        let scene = [Cell(0, 64, 0, blocks::STONE), Cell(0, 65, 0, blocks::STONE)];
        level.write_block(kept, BlockState(blocks::STONE));
        level.write_block(removed, BlockState::AIR);
        let air_written = BlockPos::new(0, 66, 0);
        level.write_block(air_written, BlockState::AIR);
        assert_eq!(
            level.changes(&scene),
            BTreeMap::from([(removed, blocks::AIR)])
        );
    }

    #[test]
    fn touching_a_player_is_not_being_in_the_way() {
        let block = [Aabb::new(3.0, 64.0, 3.0, 4.0, 65.0, 4.0)];
        // 贴着方块的东面，浮点误差只有一位。
        let touching = level_with_player(Aabb::new(
            3.999_999_999_999_999_6,
            64.0,
            3.2,
            4.6,
            65.8,
            3.8,
        ));
        assert!(touching.is_unobstructed(&block));
        let overlapping = level_with_player(Aabb::new(3.9, 64.0, 3.2, 4.5, 65.8, 3.8));
        assert!(!overlapping.is_unobstructed(&block));
    }
}
