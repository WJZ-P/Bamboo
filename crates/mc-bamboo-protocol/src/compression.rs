//! 开启压缩之后，帧里的负载换成 [`解压后长度 VarInt`][压缩数据]。
//!
//! 格式和官方 `CompressionEncoder`、`CompressionDecoder` 相同：
//! 长度是 0 时后面是原始的包，长度非 0 时后面是 zlib 压缩的包。
//! 压缩层在帧长前缀里面，所以外层帧的 21 位长度限制仍然管着压缩后的数据。

use std::io::Write;

use flate2::{Compression as Level, Decompress, FlushDecompress, write::ZlibEncoder};

use crate::{
    codec::{DecodeError, MAX_VARINT_LEN, decode_varint, encode_varint, write_varint},
    frame::{FrameError, MAX_FRAME_LEN, find_frame},
};

/// 官方 `CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH`。解压后超过这个长度的包是畸形的。
pub const MAX_UNCOMPRESSED_LEN: usize = 8_388_608;

/// 压缩层出错的原因。文案和官方的异常相同。
#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error(transparent)]
    VarInt(#[from] DecodeError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("frame batch ends in the middle of a frame")]
    IncompleteFrame,
    #[error("packet of {len} bytes is too big, the maximum is {max} bytes")]
    PacketTooBig { len: usize, max: usize },
    #[error(
        "badly compressed packet - size of {size} is below the server threshold of {threshold}"
    )]
    BelowThreshold { size: i32, threshold: i32 },
    #[error(
        "badly compressed packet - size of {size} is larger than the protocol maximum of {max}"
    )]
    UncompressedTooLong { size: i32, max: usize },
    #[error(
        "badly compressed packet - the payload inflates to {actual} bytes, not the declared {declared}"
    )]
    LengthMismatch { declared: usize, actual: usize },
    #[error("badly compressed packet - the payload is not valid zlib data")]
    InvalidData,
}

/// 按压缩帧的格式写一个包：够阈值就压缩，不够就写一个 0 长度前缀。
///
/// `threshold` 是服务端的 `network-compression-threshold`，调用方只在它非负时使用压缩层。
pub fn encode(out: &mut Vec<u8>, payload: &[u8], threshold: i32) -> Result<(), CompressionError> {
    if payload.len() > MAX_UNCOMPRESSED_LEN {
        return Err(CompressionError::PacketTooBig {
            len: payload.len(),
            max: MAX_UNCOMPRESSED_LEN,
        });
    }
    if (payload.len() as i32) < threshold {
        write_varint(out, 0);
        out.extend_from_slice(payload);
        return Ok(());
    }

    write_varint(out, payload.len() as i32);
    // 官方的 Deflater 和 flate2 的默认等级都是 6，都用 zlib 包装。
    let mut encoder = ZlibEncoder::new(Vec::new(), Level::default());
    encoder
        .write_all(payload)
        .expect("writing into a Vec cannot fail");
    let compressed = encoder.finish().expect("finishing a Vec cannot fail");
    out.extend_from_slice(&compressed);
    Ok(())
}

/// 读一个压缩帧的负载，返回解压后的包。
///
/// `threshold` 是服务端声明的阈值。和官方一样，长度非 0 但小于阈值的包直接判为畸形。
pub fn decode(payload: &[u8], threshold: i32) -> Result<Vec<u8>, CompressionError> {
    decode_limited(payload, threshold, MAX_UNCOMPRESSED_LEN)
}

/// 和 [`decode`] 相同，但解压后的长度不能超过 `max_len`，在分配内存之前就检查。
///
/// 进入 Play 之前 Bamboo 把帧限制得比官方小，压缩打开后这个限制要管到解压后的包，
/// 不然一个很小的压缩帧也能让服务端分配到协议上限那么大的内存。
pub fn decode_limited(
    payload: &[u8],
    threshold: i32,
    max_len: usize,
) -> Result<Vec<u8>, CompressionError> {
    let max_len = max_len.min(MAX_UNCOMPRESSED_LEN);
    let (declared, header_len) = decode_varint(payload)?;
    let data = &payload[header_len..];
    if declared == 0 {
        return Ok(data.to_vec());
    }
    if declared < threshold {
        return Err(CompressionError::BelowThreshold {
            size: declared,
            threshold,
        });
    }
    if declared as usize > max_len {
        return Err(CompressionError::UncompressedTooLong {
            size: declared,
            max: max_len,
        });
    }

    // 这里 declared 是正数，并且不超过协议上限。
    let declared = declared as usize;
    let mut out = vec![0; declared];
    let mut inflater = Decompress::new(true);
    inflater
        .decompress(data, &mut out, FlushDecompress::Finish)
        .map_err(|_| CompressionError::InvalidData)?;
    // 官方只比较实际解压出的长度：压缩流更长时，前面的 declared 个字节就算数。
    let actual = usize::try_from(inflater.total_out()).unwrap_or(usize::MAX);
    if actual != declared {
        return Err(CompressionError::LengthMismatch { declared, actual });
    }
    Ok(out)
}

/// 把一批未压缩的帧逐个换成压缩帧。
///
/// 世界线程按包组好批次，一次写进 socket；压缩要按包做，所以这里重新拆开每一帧。
pub fn encode_frames(frames: &[u8], threshold: i32) -> Result<Vec<u8>, CompressionError> {
    let mut out = Vec::with_capacity(frames.len());
    let mut rest = frames;
    while !rest.is_empty() {
        let Some(bounds) = find_frame(rest, MAX_FRAME_LEN)? else {
            return Err(CompressionError::IncompleteFrame);
        };
        let payload = &rest[bounds.header_len..bounds.header_len + bounds.payload_len];

        let header_start = out.len();
        out.extend_from_slice(&[0; MAX_VARINT_LEN]);
        let body_start = out.len();
        encode(&mut out, payload, threshold)?;
        let body_len = out.len() - body_start;
        if body_len > MAX_FRAME_LEN {
            return Err(FrameError::TooLong {
                len: body_len,
                max: MAX_FRAME_LEN,
            }
            .into());
        }

        let (header, header_len) = encode_varint(body_len as i32);
        out[header_start..header_start + header_len].copy_from_slice(&header[..header_len]);
        out.copy_within(body_start.., header_start + header_len);
        out.truncate(header_start + header_len + body_len);

        rest = &rest[bounds.header_len + bounds.payload_len..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个未压缩的完整帧。
    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, payload.len() as i32);
        out.extend_from_slice(payload);
        out
    }

    fn encoded(payload: &[u8], threshold: i32) -> Vec<u8> {
        let mut out = Vec::new();
        encode(&mut out, payload, threshold).unwrap();
        out
    }

    /// 逐帧拆开一批压缩帧，返回解压后的包。
    fn decode_batch(mut frames: &[u8], threshold: i32) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        while !frames.is_empty() {
            let bounds = find_frame(frames, MAX_FRAME_LEN).unwrap().unwrap();
            let end = bounds.header_len + bounds.payload_len;
            packets.push(decode(&frames[bounds.header_len..end], threshold).unwrap());
            frames = &frames[end..];
        }
        packets
    }

    #[test]
    fn packets_below_the_threshold_keep_their_bytes() {
        let payload = vec![7; 255];

        assert_eq!(encoded(&payload, 256), [&[0][..], &payload].concat());
    }

    #[test]
    fn packets_at_or_above_the_threshold_are_compressed() {
        for len in [256, 257, 1024] {
            let payload = vec![7; len];
            let compressed = encoded(&payload, 256);

            assert_ne!(compressed[0], 0, "{len} byte packet should be compressed");
            assert_eq!(decode(&compressed, 256).unwrap(), payload);
        }
    }

    #[test]
    fn a_zero_threshold_compresses_every_packet() {
        let payload = [1, 2, 3];
        let compressed = encoded(&payload, 0);

        assert_ne!(compressed[0], 0);
        assert_eq!(decode(&compressed, 0).unwrap(), payload);
    }

    #[test]
    fn the_decoder_refuses_a_compressed_packet_below_the_threshold() {
        let compressed = encoded(&[0; 10], 0);

        assert!(matches!(
            decode(&compressed, 256),
            Err(CompressionError::BelowThreshold {
                size: 10,
                threshold: 256
            })
        ));
    }

    #[test]
    fn the_decoder_refuses_a_declared_size_over_the_protocol_maximum() {
        let mut payload = Vec::new();
        write_varint(&mut payload, MAX_UNCOMPRESSED_LEN as i32 + 1);
        payload.push(0);

        assert!(matches!(
            decode(&payload, 0),
            Err(CompressionError::UncompressedTooLong { size, .. })
                if size == MAX_UNCOMPRESSED_LEN as i32 + 1
        ));
    }

    #[test]
    fn the_decoder_refuses_a_declared_size_over_the_actual_size() {
        let compressed = encoded(&[0; 10], 0);
        let (_, header_len) = decode_varint(&compressed).unwrap();
        let mut lying = Vec::new();
        write_varint(&mut lying, 11);
        lying.extend_from_slice(&compressed[header_len..]);

        assert!(matches!(
            decode(&lying, 0),
            Err(CompressionError::LengthMismatch {
                declared: 11,
                actual: 10
            })
        ));
    }

    #[test]
    fn a_limit_below_the_declared_length_rejects_before_inflating() {
        let mut frame = Vec::new();
        encode(&mut frame, &[7; 2000], 256).expect("够阈值的包能压缩");

        assert!(decode_limited(&frame, 256, 2000).is_ok());
        assert!(matches!(
            decode_limited(&frame, 256, 1024),
            Err(CompressionError::UncompressedTooLong {
                size: 2000,
                max: 1024
            })
        ));
    }

    #[test]
    fn a_stream_that_inflates_to_more_than_declared_stops_at_the_declared_length() {
        // 官方的 Inflater 只往声明大小的缓冲区里写，多出来的部分不看。
        let compressed = encoded(&[7; 20], 0);
        let (_, header_len) = decode_varint(&compressed).unwrap();
        let mut shorter = Vec::new();
        write_varint(&mut shorter, 10);
        shorter.extend_from_slice(&compressed[header_len..]);

        assert_eq!(decode(&shorter, 0).unwrap(), vec![7; 10]);
    }

    #[test]
    fn the_decoder_refuses_corrupt_zlib_data() {
        let mut payload = Vec::new();
        write_varint(&mut payload, 3);
        payload.extend_from_slice(&[0xff, 0xff, 0xff]);

        assert!(matches!(
            decode(&payload, 0),
            Err(CompressionError::InvalidData)
        ));
    }

    #[test]
    fn a_zero_length_prefix_passes_the_payload_through_unchecked() {
        // 官方看到 0 就直接放行，即使包比阈值短。
        let mut payload = vec![0];
        payload.extend_from_slice(&[1, 2, 3]);

        assert_eq!(decode(&payload, 256).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn the_decoder_refuses_a_truncated_varint() {
        assert!(matches!(
            decode(&[0x80], 0),
            Err(CompressionError::VarInt(DecodeError::UnexpectedEnd))
        ));
    }

    #[test]
    fn the_encoder_refuses_a_packet_over_the_protocol_maximum() {
        let payload = vec![0; MAX_UNCOMPRESSED_LEN + 1];

        assert!(matches!(
            encode(&mut Vec::new(), &payload, 256),
            Err(CompressionError::PacketTooBig { len, .. }) if len == MAX_UNCOMPRESSED_LEN + 1
        ));
    }

    #[test]
    fn a_batch_of_frames_is_compressed_packet_by_packet() {
        let mut batch = frame(&[1; 300]);
        batch.extend(frame(&[2; 10]));
        batch.extend(frame(&[3; 700]));

        let compressed = encode_frames(&batch, 256).unwrap();

        assert_eq!(
            decode_batch(&compressed, 256),
            [vec![1; 300], vec![2; 10], vec![3; 700]]
        );
    }

    #[test]
    fn compressing_an_empty_batch_produces_no_bytes() {
        assert!(encode_frames(&[], 256).unwrap().is_empty());
    }

    #[test]
    fn a_batch_that_ends_mid_frame_is_refused() {
        let mut batch = frame(&[1; 10]);
        batch.truncate(batch.len() - 1);

        assert!(matches!(
            encode_frames(&batch, 256),
            Err(CompressionError::IncompleteFrame)
        ));
    }
}
