//! Director 交给计算池的区块任务：解析、生成阶段、序列化。
//!
//! 任务里全是拥有的数据，不借用世界状态。回执带回任务里的盒子：生成被取消时
//! 半成品原样回来（Director 再决定丢掉还是停放），序列化不管成败都交还原盒子，
//! 盒子绝不会因为任务失败、取消或 panic 丢在池子里。
//!
//! Chunk jobs handed from the Director host to the compute pool. Jobs own their
//! data; replies always carry the box back, so a failed, cancelled or panicking
//! job never loses a column.

use std::{sync::Arc, time::Instant};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{director::SaveSeq, ticket::ChunkStatus};

use crate::{
    chunk_codec::{ChunkCodec, InlineCodec, PackedChunk},
    chunk_generator::ChunkGenerator,
    compute_pool::PoolJob,
    region::RegionColumn,
};

/// 一个区块任务。`generation` 原样带回，宿主拼 `TaskDone` 用。
pub(crate) struct ChunkJob {
    pub(crate) pos: ChunkPos,
    pub(crate) generation: u64,
    pub(crate) work: ChunkWork,
}

/// 任务的内容。
pub(crate) enum ChunkWork {
    /// 解开容器、解析两份 NBT。
    Decode { bytes: Vec<u8> },
    /// 把中心推进到 `stage`。
    Generate {
        stage: ChunkStatus,
        center: Option<Box<RegionColumn>>,
        generator: Arc<dyn ChunkGenerator>,
    },
    /// 把收回的盒子编成两份 NBT。盒子只借来读，回执里原样交还。
    Save {
        seq: SaveSeq,
        column: Box<RegionColumn>,
    },
}

/// 计算的产物，`ChunkJob::run` 的输出。
pub(crate) enum ChunkOutput {
    Decoded(Result<Box<RegionColumn>, String>),
    Generated(Box<RegionColumn>),
    Encoded(Result<PackedChunk, String>),
}

/// 送回世界线程的回执。
pub(crate) struct ChunkReply {
    pub(crate) pos: ChunkPos,
    pub(crate) generation: u64,
    pub(crate) outcome: ChunkOutcome,
}

/// 回执的内容。世界线程把它变成 `TaskResult`。
pub(crate) enum ChunkOutcome {
    Decoded(Result<Box<RegionColumn>, String>),
    Generated {
        stage: ChunkStatus,
        column: Box<RegionColumn>,
    },
    /// 生成失败（panic）：中心的半成品已经交给生成器，拿不回来了。
    GenerateFailed {
        error: String,
    },
    Saved {
        seq: SaveSeq,
        column: Box<RegionColumn>,
        encoded: Result<PackedChunk, String>,
    },
    /// 还没开始就被取消。生成任务的半成品原样带回。
    Cancelled {
        column: Option<Box<RegionColumn>>,
    },
}

/// 解开 Director 搬运的容器并解析。内联执行器和计算池共用这一份。
pub(crate) fn decode_packed(pos: ChunkPos, bytes: &[u8]) -> Result<Box<RegionColumn>, String> {
    let started = Instant::now();
    let result = PackedChunk::unpack(bytes).and_then(|packed| InlineCodec.decode(pos, &packed));
    crate::director_host::record_codec(started);
    result
}

/// 把盒子编成两份 NBT。内联执行器和计算池共用这一份。
pub(crate) fn encode_column(pos: ChunkPos, column: &RegionColumn) -> Result<PackedChunk, String> {
    let started = Instant::now();
    let result = InlineCodec.encode(pos, column);
    crate::director_host::record_codec(started);
    result
}

impl PoolJob for ChunkJob {
    type Output = ChunkOutput;
    type Reply = ChunkReply;

    fn run(&mut self) -> ChunkOutput {
        let pos = self.pos;
        match &mut self.work {
            ChunkWork::Decode { bytes } => ChunkOutput::Decoded(decode_packed(pos, bytes)),
            ChunkWork::Generate {
                stage,
                center,
                generator,
            } => ChunkOutput::Generated(generator.generate(*stage, pos, center.take(), &[])),
            ChunkWork::Save { column, .. } => ChunkOutput::Encoded(encode_column(pos, column)),
        }
    }

    fn finish(self, output: Result<ChunkOutput, String>) -> ChunkReply {
        let outcome = match (self.work, output) {
            (ChunkWork::Decode { .. }, Ok(ChunkOutput::Decoded(result))) => {
                ChunkOutcome::Decoded(result)
            }
            (ChunkWork::Decode { .. }, Err(error)) => ChunkOutcome::Decoded(Err(error)),
            (ChunkWork::Generate { stage, .. }, Ok(ChunkOutput::Generated(column))) => {
                ChunkOutcome::Generated { stage, column }
            }
            (ChunkWork::Generate { .. }, Err(error)) => ChunkOutcome::GenerateFailed { error },
            (ChunkWork::Save { seq, column }, Ok(ChunkOutput::Encoded(encoded))) => {
                ChunkOutcome::Saved {
                    seq,
                    column,
                    encoded,
                }
            }
            (ChunkWork::Save { seq, column }, Err(error)) => ChunkOutcome::Saved {
                seq,
                column,
                encoded: Err(error),
            },
            // `run` 对每种任务只产出对应的那种输出，走不到这里；真走到了也不丢盒子。
            (ChunkWork::Save { seq, column }, Ok(_)) => ChunkOutcome::Saved {
                seq,
                column,
                encoded: Err("mismatched compute output".to_owned()),
            },
            (ChunkWork::Decode { .. }, Ok(_)) => {
                ChunkOutcome::Decoded(Err("mismatched compute output".to_owned()))
            }
            (ChunkWork::Generate { .. }, Ok(_)) => ChunkOutcome::GenerateFailed {
                error: "mismatched compute output".to_owned(),
            },
        };
        ChunkReply {
            pos: self.pos,
            generation: self.generation,
            outcome,
        }
    }

    fn cancelled(self) -> ChunkReply {
        let outcome = match self.work {
            ChunkWork::Decode { .. } => ChunkOutcome::Cancelled { column: None },
            ChunkWork::Generate { center, .. } => ChunkOutcome::Cancelled { column: center },
            // 宿主从不取消存盘；万一取消了，照样编码，盒子和快照都不丢。
            ChunkWork::Save { seq, column } => {
                let encoded = encode_column(self.pos, &column);
                ChunkOutcome::Saved {
                    seq,
                    column,
                    encoded,
                }
            }
        };
        ChunkReply {
            pos: self.pos,
            generation: self.generation,
            outcome,
        }
    }
}
