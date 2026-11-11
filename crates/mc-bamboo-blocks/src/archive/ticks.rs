//! `ChunkTicks` ⇄ 两条 `SavedTick` 列表，见 `docs/m3-storage.md` 5.5。
//!
//! `SavedTick.codec` 的键是 `i`（注册名字符串）、`x/y/z`（int）、`t`（int 相对延迟）、
//! `p`（int 优先级 -3..3）。`LevelChunkTicks.unpack` 给读进来的刻分配从 `-len` 开始的
//! 负数子序号；Bamboo 的 `BlockTicks::insert_chunk` 已经按盒子的列表顺序做同样的事。

use mc_bamboo_data::{blocks, registries::fluid};
use mc_bamboo_nbt::Tag;

use super::tag::{self, Error, Result, put};
use crate::{ChunkTicks, TickPriority};

/// 一个区块的两条计划刻列表。`None` 表示这个列表缺了（按官方按空列表处理）。
pub struct TickLists {
    pub blocks: Vec<Tag>,
    pub fluids: Vec<Tag>,
    /// `t` 被饱和到 i32 范围的条目数，交给调用方记日志。
    pub clamped: usize,
}

/// 把 `ChunkTicks` 编成两条 NBT 列表。`delay` 超出 i32 时饱和并报告，不回绕。
pub fn encode_chunk_ticks(ticks: &ChunkTicks) -> Result<TickLists> {
    let mut out = TickLists {
        blocks: Vec::new(),
        fluids: Vec::new(),
        clamped: 0,
    };
    for (pos, block, priority, delay) in ticks.block_ticks() {
        let name = blocks::BLOCKS
            .get(block)
            .map(|block| block.name)
            .ok_or_else(|| Error::new(format!("unknown block id {block}")))?;
        out.blocks
            .push(saved_tick(name, pos, priority, delay, &mut out.clamped));
    }
    for (pos, fluid_id, priority, delay) in ticks.fluid_ticks() {
        let name = fluid::name_of(fluid_id)
            .ok_or_else(|| Error::new(format!("unknown fluid id {fluid_id}")))?;
        out.fluids
            .push(saved_tick(name, pos, priority, delay, &mut out.clamped));
    }
    Ok(out)
}

/// 把两条 NBT 列表读回 `ChunkTicks`。不认识的 `i` 和坏条目跳过并报告；
/// 解码后的列表顺序就是子序号的顺序。
pub fn decode_chunk_ticks(blocks: &Tag, fluids: &Tag) -> Result<(ChunkTicks, Vec<String>)> {
    let mut problems = Vec::new();
    let mut decoded = ChunkTicks::empty();
    if let Tag::List(values) = blocks {
        for (index, value) in values.iter().enumerate() {
            match decode_saved_tick(value, true) {
                Ok(Some(tick)) => decoded.push_block(tick),
                Ok(None) => problems.push(format!("block_ticks[{index}] has an unknown id")),
                Err(error) => problems.push(format!("block_ticks[{index}]: {}", error.0)),
            }
        }
    } else {
        problems.push("block_ticks is not a list".to_owned());
    }
    if let Tag::List(values) = fluids {
        for (index, value) in values.iter().enumerate() {
            match decode_saved_tick(value, false) {
                Ok(Some(tick)) => decoded.push_fluid(tick),
                Ok(None) => problems.push(format!("fluid_ticks[{index}] has an unknown id")),
                Err(error) => problems.push(format!("fluid_ticks[{index}]: {}", error.0)),
            }
        }
    } else {
        problems.push("fluid_ticks is not a list".to_owned());
    }
    Ok((decoded, problems))
}

fn saved_tick(
    name: &str,
    pos: mc_bamboo_core::BlockPos,
    priority: TickPriority,
    delay: i64,
    clamped: &mut usize,
) -> Tag {
    let delay = match i32::try_from(delay) {
        Ok(value) => value,
        Err(_) => {
            *clamped += 1;
            if delay < 0 { i32::MIN } else { i32::MAX }
        }
    };
    let mut out = Vec::new();
    put(&mut out, "i", Tag::String(name.to_owned()));
    put(&mut out, "x", Tag::Int(pos.x));
    put(&mut out, "y", Tag::Int(pos.y));
    put(&mut out, "z", Tag::Int(pos.z));
    put(&mut out, "t", Tag::Int(delay));
    put(&mut out, "p", Tag::Int(priority as i32));
    Tag::Compound(out)
}

/// `SavedTick<T>` 的中立形式，供 `ChunkTicks::push_*` 使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedEntry {
    Block {
        pos: mc_bamboo_core::BlockPos,
        block: usize,
        priority: TickPriority,
        delay: i64,
    },
    Fluid {
        pos: mc_bamboo_core::BlockPos,
        fluid: i32,
        priority: TickPriority,
        delay: i64,
    },
}

fn decode_saved_tick(tag: &Tag, is_block: bool) -> Result<Option<SavedEntry>> {
    let name = tag::string_or(tag, "i").ok_or_else(|| Error::new("saved tick has no i"))?;
    let pos = mc_bamboo_core::BlockPos::new(
        tag::int_or(tag, "x", 0),
        tag::int_or(tag, "y", 0),
        tag::int_or(tag, "z", 0),
    );
    let delay = i64::from(tag::int_or(tag, "t", 0));
    let priority = priority_from_int(tag::int_or(tag, "p", 0));
    Ok(if is_block {
        blocks::BLOCKS
            .iter()
            .position(|block| block.name == name)
            .map(|block| SavedEntry::Block {
                pos,
                block,
                priority,
                delay,
            })
    } else {
        fluid::by_name(name).map(|fluid| SavedEntry::Fluid {
            pos,
            fluid,
            priority,
            delay,
        })
    })
}

/// `TickPriority.byValue` 会把越界值折到两端。
fn priority_from_int(value: i32) -> TickPriority {
    match value {
        ..=-3 => TickPriority::ExtremelyHigh,
        -2 => TickPriority::VeryHigh,
        -1 => TickPriority::High,
        0 => TickPriority::Normal,
        1 => TickPriority::Low,
        2 => TickPriority::VeryLow,
        3.. => TickPriority::ExtremelyLow,
    }
}
