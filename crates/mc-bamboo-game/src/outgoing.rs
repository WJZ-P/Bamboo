//! 世界线程交给连接层的一批帧。
//!
//! 普通帧按连接压缩；区块这种同一份内容发给很多人的帧做成 [`SharedFrames`]，
//! 在连接任务里第一次用到时才压缩一次，别的连接直接借那份字节。片段按进入
//! [`Outgoing`] 的先后写出，连接层不能重排。

use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
};

use mc_bamboo_protocol::compression::{self, CompressionError};

/// 一个或多个完整的未压缩帧，供多条连接共用。
///
/// 压缩在长度前缀里面、按包进行，加密在最外面按连接进行，所以压好的字节对所有连接都一样。
#[derive(Debug)]
pub struct SharedFrames {
    raw: Vec<u8>,
    /// 第一次压缩时记下阈值和结果。所有连接用同一个配置，阈值不同的那次现场压，不覆盖缓存。
    compressed: OnceLock<(i32, Vec<u8>)>,
}

impl SharedFrames {
    pub fn new(raw: Vec<u8>) -> Self {
        Self {
            raw,
            compressed: OnceLock::new(),
        }
    }

    /// 未压缩的完整帧。
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// 按 `threshold` 压缩后的完整帧。缓存命中时直接借出，不复制。
    pub fn compressed(&self, threshold: i32) -> Result<Cow<'_, [u8]>, CompressionError> {
        if let Some((cached_threshold, bytes)) = self.compressed.get() {
            if *cached_threshold == threshold {
                return Ok(Cow::Borrowed(bytes));
            }
            return Ok(Cow::Owned(compression::encode_frames(
                &self.raw, threshold,
            )?));
        }

        // 压缩失败不写缓存，下一次可以重试。
        let bytes = compression::encode_frames(&self.raw, threshold)?;
        match self.compressed.set((threshold, bytes)) {
            Ok(()) => {
                let (_, cached) = self.compressed.get().expect("刚写进去");
                Ok(Cow::Borrowed(cached))
            }
            // 别人抢先写进去了。阈值不同时自己手里的字节正是这个阈值要的，不必再压一次。
            Err((_, bytes)) => {
                let (cached_threshold, cached) = self.compressed.get().expect("已被占用");
                if *cached_threshold == threshold {
                    Ok(Cow::Borrowed(cached))
                } else {
                    Ok(Cow::Owned(bytes))
                }
            }
        }
    }
}

/// 一批帧里的一段。
#[derive(Debug)]
pub enum Segment {
    /// 普通帧，连接层照旧压缩。
    Raw(Vec<u8>),
    /// 多共用的帧，压缩结果也能共用。
    Shared(Arc<SharedFrames>),
}

/// 一 tick 里写给一名玩家的帧，按段分开放。
///
/// 相邻的原始帧合进同一段，共享帧各自成段；段写到线上的顺序就是它们进来的顺序。
#[derive(Debug, Default)]
pub struct Outgoing {
    segments: Vec<Segment>,
}

impl Outgoing {
    /// 只有一段普通帧的批次，连接任务自己发的包用这个。
    pub fn from_raw(frames: Vec<u8>) -> Self {
        Self {
            segments: vec![Segment::Raw(frames)],
        }
    }

    /// 追加普通帧到末尾的 `Raw` 段，末尾不是 `Raw` 就新开一段。
    pub fn push_raw(&mut self, frames: &[u8]) {
        self.raw_buffer().extend_from_slice(frames);
    }

    /// 末尾的 `Raw` 段，给 `append_frame` 之类的追加写入用。
    pub fn raw_buffer(&mut self) -> &mut Vec<u8> {
        if !matches!(self.segments.last(), Some(Segment::Raw(_))) {
            self.segments.push(Segment::Raw(Vec::new()));
        }
        let Some(Segment::Raw(raw)) = self.segments.last_mut() else {
            unreachable!("上面刚放好一个 Raw 段")
        };
        raw
    }

    pub fn push_shared(&mut self, frames: Arc<SharedFrames>) {
        self.segments.push(Segment::Shared(frames));
    }

    /// 没有任何字节要写。
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// 各段未压缩字节数之和，出站队列按这个数记账。
    pub fn len_bytes(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Raw(frames) => frames.len(),
                Segment::Shared(frames) => frames.raw().len(),
            })
            .sum()
    }

    /// 按顺序的各段。
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// 按顺序把各段的未压缩字节拼起来。给只看线上字节的调用方（测试、录制）用。
    pub fn to_raw_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len_bytes());
        for segment in &self.segments {
            match segment {
                Segment::Raw(frames) => out.extend_from_slice(frames),
                Segment::Shared(frames) => out.extend_from_slice(frames.raw()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    use mc_bamboo_protocol::{
        codec::decode_varint,
        compression::{self, CompressionError, encode_frames},
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{
            ClientboundPacket,
            play::{ChunkBatchFinished, KeepAlive},
        },
    };

    use super::*;

    /// 内容是 `len` 个字节的完整帧。
    fn frame(byte: u8, len: usize) -> Vec<u8> {
        struct Filler(u8, usize);
        impl ClientboundPacket for Filler {
            const ID: i32 = 0x2a;

            fn write_body(&self, out: &mut Vec<u8>) {
                out.resize(out.len() + self.1, self.0);
            }
        }
        let mut out = Vec::new();
        mc_bamboo_protocol::frame::append_frame(&mut out, &Filler(byte, len)).unwrap();
        out
    }

    fn batch() -> Vec<u8> {
        [frame(1, 300), frame(2, 10), frame(3, 700)].concat()
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn shared_frames_are_send_and_sync() {
        assert_send_sync::<SharedFrames>();
        assert_send_sync::<Outgoing>();
    }

    #[test]
    fn compressed_bytes_match_the_unshared_encoder() {
        let raw = batch();
        let frames = SharedFrames::new(raw.clone());

        for threshold in [0, 256, 1024] {
            assert_eq!(
                *frames.compressed(threshold).unwrap(),
                *encode_frames(&raw, threshold).unwrap()
            );
        }
    }

    #[test]
    fn repeated_calls_borrow_the_same_bytes() {
        let frames = SharedFrames::new(batch());
        let first = frames.compressed(256).unwrap();
        let second = frames.compressed(256).unwrap();

        assert!(matches!(first, Cow::Borrowed(_)));
        assert!(std::ptr::eq(first.as_ptr(), second.as_ptr()));
    }

    #[test]
    fn a_different_threshold_does_not_overwrite_the_cache() {
        let raw = batch();
        let frames = SharedFrames::new(raw.clone());
        let cached = frames.compressed(256).unwrap();
        let cached_ptr = cached.as_ptr();

        let other = frames.compressed(512).unwrap();
        assert!(matches!(other, Cow::Owned(_)));
        assert_eq!(*other, *encode_frames(&raw, 512).unwrap());

        let again = frames.compressed(256).unwrap();
        assert!(std::ptr::eq(cached_ptr, again.as_ptr()));
        assert_eq!(*again, *encode_frames(&raw, 256).unwrap());
    }

    #[test]
    fn concurrent_calls_end_up_borrowing_one_shared_buffer() {
        let frames = SharedFrames::new(batch());
        let start = Barrier::new(8);
        thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        let compressed = frames.compressed(256).unwrap();
                        assert!(matches!(compressed, Cow::Borrowed(_)));
                        compressed.as_ptr() as usize
                    })
                })
                .collect();
            let pointers: Vec<usize> = handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect();
            assert!(pointers.iter().all(|&pointer| pointer == pointers[0]));
        });
        assert_eq!(
            *frames.compressed(256).unwrap(),
            *encode_frames(frames.raw(), 256).unwrap()
        );
    }

    #[test]
    fn a_failed_compression_is_not_cached() {
        let mut raw = frame(1, 10);
        raw.truncate(raw.len() - 1);
        let frames = SharedFrames::new(raw.clone());

        for _ in 0..2 {
            assert!(matches!(
                frames.compressed(0),
                Err(CompressionError::IncompleteFrame)
            ));
            assert!(frames.compressed.get().is_none());
        }
    }

    #[test]
    fn segments_keep_their_order_and_merge_adjacent_raw_writes() {
        let shared = Arc::new(SharedFrames::new(frame(9, 20)));
        let mut outgoing = Outgoing::default();
        assert!(outgoing.is_empty());

        outgoing.push_raw(&frame(1, 3));
        mc_bamboo_protocol::frame::append_frame(outgoing.raw_buffer(), &KeepAlive { id: 7 })
            .unwrap();
        outgoing.push_shared(Arc::clone(&shared));
        outgoing.push_raw(&frame(2, 4));
        outgoing.push_shared(Arc::clone(&shared));
        outgoing.push_raw(&frame(3, 5));

        let expected = [
            frame(1, 3),
            frame_of(&KeepAlive { id: 7 }),
            shared.raw().to_vec(),
            frame(2, 4),
            shared.raw().to_vec(),
            frame(3, 5),
        ]
        .concat();
        assert_eq!(outgoing.to_raw_bytes(), expected);
        assert_eq!(outgoing.len_bytes(), expected.len());
        assert!(!outgoing.is_empty());

        let segments = outgoing.segments();
        assert_eq!(segments.len(), 5);
        assert!(matches!(segments[0], Segment::Raw(_)));
        assert!(matches!(segments[1], Segment::Shared(_)));
        assert!(matches!(segments[2], Segment::Raw(_)));
        assert!(matches!(segments[3], Segment::Shared(_)));
        assert!(matches!(segments[4], Segment::Raw(_)));
        // 前两个普通帧相邻，合在同一段里。
        let Segment::Raw(first) = &segments[0] else {
            panic!("第一段是普通帧")
        };
        assert_eq!(
            *first,
            [frame(1, 3), frame_of(&KeepAlive { id: 7 })].concat()
        );
    }

    fn frame_of<P: ClientboundPacket>(packet: &P) -> Vec<u8> {
        let mut out = Vec::new();
        mc_bamboo_protocol::frame::append_frame(&mut out, packet).unwrap();
        out
    }

    #[test]
    fn to_raw_bytes_is_what_the_old_flat_buffer_held() {
        let mut old = Vec::new();
        let mut outgoing = Outgoing::default();
        let packets = [
            frame_of(&KeepAlive { id: 1 }),
            frame_of(&ChunkBatchFinished { batch_size: 3 }),
        ];
        for frames in packets {
            old.extend_from_slice(&frames);
            outgoing.push_raw(&frames);
        }
        let shared = frame(5, 100);
        old.extend_from_slice(&shared);
        outgoing.push_shared(Arc::new(SharedFrames::new(shared.clone())));
        let last = frame_of(&KeepAlive { id: 9 });
        old.extend_from_slice(&last);
        outgoing.push_raw(&last);

        assert_eq!(outgoing.to_raw_bytes(), old);
        // 旧缓冲里的每个包都还是能按帧拆开的完整包。
        let mut rest = old.as_slice();
        let mut count = 0;
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN).unwrap().unwrap();
            let end = bounds.header_len + bounds.payload_len;
            let (_, header_len) = decode_varint(&rest[bounds.header_len..end]).unwrap();
            assert!(header_len > 0);
            rest = &rest[end..];
            count += 1;
        }
        assert_eq!(count, 4);
    }

    #[test]
    fn from_raw_moves_a_connection_tasks_batch() {
        let raw = batch();
        let outgoing = Outgoing::from_raw(raw.clone());
        assert_eq!(outgoing.to_raw_bytes(), raw);
        assert_eq!(outgoing.len_bytes(), raw.len());

        // 压缩后的字节和旧办法整块压缩一致。
        assert_eq!(
            *compression::encode_frames(&outgoing.to_raw_bytes(), 256).unwrap(),
            *encode_frames(&raw, 256).unwrap()
        );
    }
}
