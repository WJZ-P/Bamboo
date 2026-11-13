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

#[cfg(test)]
mod bench_tests {
    use std::{
        collections::BTreeMap,
        time::{Duration, Instant},
    };

    use mc_bamboo_data::blocks;
    use mc_bamboo_world::column::ChunkColumn;

    use super::*;
    use crate::{
        compute_pool::ComputePool,
        region::{RegionColumn, RegionColumnParts},
    };

    /// 一个改过方块的区块：几层石头里散着几种方块，让调色板和 NBT 都不是模板那份。
    fn edited_column(seed: i32) -> Box<RegionColumn> {
        let mut column = ChunkColumn::from_template();
        let chunk = column.chunk_mut();
        let palette = [
            blocks::STONE,
            blocks::GLOWSTONE,
            blocks::DIAMOND_BLOCK,
            blocks::GOLD_BLOCK,
        ];
        for y in -59..-20 {
            for x in 0..16 {
                for z in 0..16 {
                    let index = (x as i32 * 7 + z as i32 * 13 + y * 3 + seed).rem_euclid(4);
                    chunk.set_block(x, y, z, palette[index as usize]);
                }
            }
        }
        RegionColumn::from_parts(RegionColumnParts {
            column: Box::new(column),
            ticks: None,
            block_entities: Default::default(),
            redstone: Default::default(),
            containers: BTreeMap::new(),
            block_items: BTreeMap::new(),
            command_blocks: BTreeMap::new(),
            books: BTreeMap::new(),
            entities: Default::default(),
        })
    }

    /// 跑一批任务。`pool` 为 `None` 时内联。返回（世界线程花在提交和收回上的时间, 总墙钟时间）。
    fn run_batch(
        pool: Option<&mut ComputePool<ChunkJob>>,
        jobs: Vec<ChunkJob>,
    ) -> (Duration, Duration) {
        let started = Instant::now();
        match pool {
            None => {
                for mut job in jobs {
                    let output = job.run();
                    let _ = job.finish(Ok(output));
                }
                let elapsed = started.elapsed();
                (elapsed, elapsed)
            }
            Some(pool) => {
                let count = jobs.len();
                for (id, job) in jobs.into_iter().enumerate() {
                    pool.submit(id as u64, 0, job);
                }
                let mut world = started.elapsed();
                let mut received = 0;
                while received < count {
                    // 等待不算世界线程的时间：正式运行时世界线程不等，下一刻开头才收。
                    let Some(_) = pool.recv_timeout(Duration::from_secs(10)) else {
                        panic!("计算池 10 秒内没有回执");
                    };
                    let collect = Instant::now();
                    received += 1;
                    while pool.try_recv().is_some() {
                        received += 1;
                    }
                    world += collect.elapsed();
                }
                (world, started.elapsed())
            }
        }
    }

    /// 编解码吞吐：一批改过方块的区块，内联 vs 4 线程，序列化和解析各量一次。
    /// 只打印数字，不断言快慢：机器上同时跑别的东西时数字会有噪声。
    ///
    /// `cargo test --release -p mc-bamboo-game --lib codec_throughput -- --ignored --nocapture`
    #[test]
    #[ignore = "基准，手动跑"]
    fn codec_throughput_inline_vs_pool() {
        const CHUNKS: i32 = 512;
        let columns: Vec<Box<RegionColumn>> = (0..CHUNKS).map(edited_column).collect();
        let packed: Vec<Vec<u8>> = columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                encode_column(ChunkPos::new(i as i32, 0), column)
                    .expect("能编码")
                    .pack()
            })
            .collect();
        let average_bytes = packed.iter().map(Vec::len).sum::<usize>() / packed.len();
        println!("{CHUNKS} 个区块，平均每个 {average_bytes} 字节未压缩 NBT");

        // 盒子不能复制：每一批现造，造的时间不算进计时。
        let encode_jobs = |_: &[Box<RegionColumn>]| -> Vec<ChunkJob> {
            (0..CHUNKS)
                .map(|i| ChunkJob {
                    pos: ChunkPos::new(i, 0),
                    generation: 1,
                    work: ChunkWork::Save {
                        seq: i as u64,
                        column: edited_column(i),
                    },
                })
                .collect()
        };
        let decode_jobs = |packed: &[Vec<u8>]| -> Vec<ChunkJob> {
            packed
                .iter()
                .enumerate()
                .map(|(i, bytes)| ChunkJob {
                    pos: ChunkPos::new(i as i32, 0),
                    generation: 1,
                    work: ChunkWork::Decode {
                        bytes: bytes.clone(),
                    },
                })
                .collect()
        };

        let mut pool = ComputePool::new("bench-chunk-worker", 4).expect("能起线程");
        for (name, kind) in [("序列化", 0), ("解析", 1)] {
            let build = || {
                if kind == 0 {
                    encode_jobs(&columns)
                } else {
                    decode_jobs(&packed)
                }
            };
            // 先各热身一遍。
            let _ = run_batch(None, build());
            let _ = run_batch(Some(&mut pool), build());
            let (inline_world, inline_wall) = run_batch(None, build());
            let (pool_world, pool_wall) = run_batch(Some(&mut pool), build());
            let per = |d: Duration| d.as_secs_f64() * 1e6 / f64::from(CHUNKS);
            println!(
                "{name}：内联 世界线程 {:.1} ms / 墙钟 {:.1} ms（{:.1} us/区块）；\
                 4 线程 世界线程 {:.1} ms / 墙钟 {:.1} ms（{:.1} us/区块）",
                inline_world.as_secs_f64() * 1e3,
                inline_wall.as_secs_f64() * 1e3,
                per(inline_wall),
                pool_world.as_secs_f64() * 1e3,
                pool_wall.as_secs_f64() * 1e3,
                per(pool_wall),
            );
        }
    }
}
