//! Envelope codec: magic + u32 unpacked size + zlib stream.
//!
//! The same layout serves the clipboard and `.dms` files
//! (`PortalSequenceData`) and `.dmp` part files (`PortalPartData`).
//!
//! Speed: decompression goes straight into a buffer of the declared size.
//! Compression splits the data into blocks that are deflated on all cores
//! and joined into one valid zlib stream (each block ends on a full flush, as
//! `pigz` does); the Adler-32 checksums of the blocks are combined.

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Magic {
    PortalSequenceData,
    PortalPartData,
}

impl Magic {
    pub const fn as_bytes(self) -> &'static [u8] {
        match self {
            Magic::PortalSequenceData => b"PortalSequenceData",
            Magic::PortalPartData => b"PortalPartData",
        }
    }

    pub const fn header_len(self) -> usize {
        self.as_bytes().len() + 4
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Input too short to hold the header, or wrong magic.
    BadMagic,
    /// The zlib stream failed to decompress.
    Zlib(String),
    /// Decompressed length differs from the declared `unpacked_size`.
    SizeMismatch { declared: u32, actual: usize },
}

pub struct Envelope {
    pub magic: Magic,
    pub unpacked_size: u32,
    pub data: Vec<u8>,
}

/// Decode an envelope. Bytes after the end of the zlib stream are ignored
/// (Windows clipboard blocks can be padded).
pub fn decode(bytes: &[u8], magic: Magic) -> Result<Envelope, EnvelopeError> {
    let header = magic.header_len();
    if bytes.len() < header || &bytes[..magic.as_bytes().len()] != magic.as_bytes() {
        return Err(EnvelopeError::BadMagic);
    }
    let declared = u32::from_le_bytes(bytes[magic.as_bytes().len()..header].try_into().unwrap());
    let input = &bytes[header..];
    if let Some(data) = parallel_inflate(input, declared as usize) {
        return Ok(Envelope { magic, unpacked_size: declared, data });
    }
    // deflate expands at most ~1032:1, so a bogus size can't force a huge allocation
    let mut data = Vec::with_capacity((declared as usize).min(input.len().saturating_mul(1032) + 64) + 1);
    let mut z = Decompress::new(true);
    loop {
        let consumed = z.total_in() as usize;
        let status = z
            .decompress_vec(&input[consumed..], &mut data, FlushDecompress::Finish)
            .map_err(|e| EnvelopeError::Zlib(e.to_string()))?;
        match status {
            Status::StreamEnd => break,
            _ if data.len() == data.capacity() => {
                if data.len() > declared as usize {
                    return Err(EnvelopeError::SizeMismatch { declared, actual: data.len() });
                }
                data.reserve(data.len().max(1 << 16));
            }
            _ => return Err(EnvelopeError::Zlib("the stream ends early".into())),
        }
    }
    if data.len() != declared as usize {
        return Err(EnvelopeError::SizeMismatch { declared, actual: data.len() });
    }
    Ok(Envelope { magic, unpacked_size: declared, data })
}

/// Inflate a stream written by [`encode`] on all cores: its blocks were
/// compressed independently and each ends on a full flush (`00 00 FF FF`),
/// so each can be inflated on its own. Every block must produce exactly
/// `BLOCK` bytes (the last one the rest) and the combined Adler-32 must
/// match; anything else (e.g. a stream from Domino itself, or a flush
/// pattern that happens to occur inside compressed data) returns `None`
/// and the caller inflates sequentially.
fn parallel_inflate(input: &[u8], declared: usize) -> Option<Vec<u8>> {
    let blocks = declared.div_ceil(BLOCK);
    if blocks < 2 || input.len() < 6 || input[0] & 0x0F != 8 || input[1] & 0x20 != 0 {
        return None;
    }
    let deflate = &input[2..];
    // block starts: right after each full-flush marker
    let span = deflate.len().div_ceil(rayon::current_num_threads() * 4).max(1 << 16);
    let mut starts: Vec<usize> = (0..deflate.len().div_ceil(span))
        .into_par_iter()
        .flat_map_iter(|c| {
            let from = c * span;
            let to = (from + span + 3).min(deflate.len());
            deflate[from..to]
                .windows(4)
                .enumerate()
                .filter(|(_, w)| *w == [0, 0, 0xFF, 0xFF])
                .map(move |(i, _)| from + i + 4)
                .filter(move |&p| p - 4 < from + span)
                .collect::<Vec<_>>()
        })
        .collect();
    starts.insert(0, 0);
    if starts.len() != blocks {
        return None;
    }

    let mut data = vec![0u8; declared];
    let ends: Vec<usize> = starts[1..].iter().copied().chain([deflate.len()]).collect();
    let results: Vec<Option<(u32, usize)>> = data
        .par_chunks_mut(BLOCK)
        .zip(starts.par_iter().zip(ends.par_iter()))
        .enumerate()
        .map(|(k, (out, (&from, &to)))| {
            let last = k == blocks - 1;
            let mut z = Decompress::new(false);
            let status = z.decompress(&deflate[from..to], out, FlushDecompress::None).ok()?;
            let complete = z.total_out() as usize == out.len()
                && if last { status == Status::StreamEnd } else { z.total_in() as usize == to - from };
            complete.then(|| (adler32(out), from + z.total_in() as usize))
        })
        .collect();
    let results: Vec<(u32, usize)> = results.into_iter().collect::<Option<_>>()?;
    let checksum = results.iter().enumerate().skip(1).fold(results[0].0, |acc, (k, (a, _))| {
        let len = if k == blocks - 1 { declared - k * BLOCK } else { BLOCK };
        adler32_combine(acc, *a, len)
    });
    let at = results[blocks - 1].1;
    let stored = deflate.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))?;
    (stored == checksum).then_some(data)
}

/// Blocks compressed independently (bigger blocks compress a little better).
const BLOCK: usize = 1 << 20;

/// zlib level 1: on this data it is ~4–5× faster than the default level and
/// the output is only ~15% larger.
const LEVEL: u32 = 1;

pub fn encode(data: &[u8], magic: Magic) -> Vec<u8> {
    let mut out = Vec::with_capacity(magic.header_len() + data.len() / 3 + 64);
    encode_streamed(data.len(), magic, |at, buf| buf.copy_from_slice(&data[at..at + buf.len()]), |b| {
        out.extend_from_slice(b);
        Ok(())
    })
    .expect("writing to memory cannot fail");
    out
}

/// Encode a stream of `len` bytes that `fill(offset, buf)` produces on
/// demand, passing the envelope to `sink` piece by piece. Blocks are
/// produced and compressed a few dozen at a time on all cores, so memory
/// stays at a few dozen megabytes whatever `len` is.
pub fn encode_streamed(
    len: usize,
    magic: Magic,
    fill: impl Fn(usize, &mut [u8]) + Sync,
    mut sink: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> std::io::Result<()> {
    sink(magic.as_bytes())?;
    sink(&(len as u32).to_le_bytes())?;
    sink(&[0x78, 0x01])?; // zlib header: deflate, 32K window, fastest
    let blocks = len.div_ceil(BLOCK).max(1);
    let window = rayon::current_num_threads() * 2;
    let mut checksum = 1u32; // Adler-32 of nothing
    for first in (0..blocks).step_by(window) {
        let packed: Vec<(Vec<u8>, u32, usize)> = (first..(first + window).min(blocks))
            .into_par_iter()
            .map(|k| {
                let size = BLOCK.min(len - k * BLOCK);
                let mut buf = vec![0u8; size];
                fill(k * BLOCK, &mut buf);
                (deflate_block(&buf, k == blocks - 1), adler32(&buf), size)
            })
            .collect();
        for (bytes, adler, size) in packed {
            sink(&bytes)?;
            checksum = adler32_combine(checksum, adler, size);
        }
    }
    sink(&checksum.to_be_bytes())
}

/// Raw deflate of one block; non-final blocks end on a full flush so the
/// next block can follow them in the same stream.
fn deflate_block(block: &[u8], last: bool) -> Vec<u8> {
    let mut z = Compress::new(Compression::new(LEVEL), false);
    let mut out = Vec::with_capacity(block.len() / 3 + 1024);
    let flush = if last { FlushCompress::Finish } else { FlushCompress::Full };
    loop {
        let consumed = z.total_in() as usize;
        let status = z.compress_vec(&block[consumed..], &mut out, flush).expect("deflate cannot fail on valid input");
        let done = z.total_in() as usize == block.len() && out.len() < out.capacity();
        if status == Status::StreamEnd || (done && !last) {
            break;
        }
        out.reserve(out.capacity().max(4096));
    }
    out
}

const MOD: u32 = 65521;

/// Adler-32 (RFC 1950), processed in runs short enough that the sums can't
/// overflow before the modulo.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for run in data.chunks(5552) {
        for &x in run {
            a += x as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// Checksum of `A ‖ B` from the checksums of A and of B (B is `len_b` long).
pub fn adler32_combine(adler_a: u32, adler_b: u32, len_b: usize) -> u32 {
    let rem = (len_b % MOD as usize) as u64;
    let m = MOD as u64;
    let (a1, b1) = ((adler_a & 0xFFFF) as u64, (adler_a >> 16) as u64);
    let (a2, b2) = ((adler_b & 0xFFFF) as u64, (adler_b >> 16) as u64);
    let a = (a1 + a2 + m - 1) % m;
    let b = (rem * a1 % m + b1 + b2 + m - rem) % m;
    ((b << 16) | a) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    #[test]
    fn round_trips() {
        let payload = b"hello domino".repeat(10);
        let env = encode(&payload, Magic::PortalSequenceData);
        let decoded = decode(&env, Magic::PortalSequenceData).unwrap();
        assert_eq!(decoded.data, payload);
        assert_eq!(decoded.unpacked_size, payload.len() as u32);
    }

    #[test]
    fn multi_block_stream_is_standard_zlib() {
        let payload: Vec<u8> = (0..(3 * BLOCK + 12345)).map(|i| (i * 7 % 251) as u8).collect();
        let env = encode(&payload, Magic::PortalSequenceData);
        // any zlib decoder must accept it, checksum included
        let mut out = Vec::new();
        ZlibDecoder::new(&env[22..]).read_to_end(&mut out).unwrap();
        assert_eq!(out, payload);
        assert_eq!(decode(&env, Magic::PortalSequenceData).unwrap().data, payload);
        assert!(encode(&[], Magic::PortalSequenceData).len() > 22);
        assert!(decode(&encode(&[], Magic::PortalSequenceData), Magic::PortalSequenceData).unwrap().data.is_empty());
    }

    #[test]
    fn parallel_inflate_takes_our_streams_and_declines_others() {
        let payload: Vec<u8> = (0..(5 * BLOCK + 777)).map(|i| ((i / 3) * 7 % 253) as u8).collect();
        let env = encode(&payload, Magic::PortalSequenceData);
        assert_eq!(parallel_inflate(&env[22..], payload.len()).as_deref(), Some(&payload[..]));
        // a stream from another compressor: one block, so no split points
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &payload).unwrap();
        let other = z.finish().unwrap();
        assert!(parallel_inflate(&other, payload.len()).is_none());
        let mut env2 = b"PortalSequenceData".to_vec();
        env2.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        env2.extend_from_slice(&other);
        assert_eq!(decode(&env2, Magic::PortalSequenceData).unwrap().data, payload);
        // a damaged checksum is caught on the parallel path too
        let mut bad = env.clone();
        let n = bad.len();
        bad[n - 2] ^= 1;
        assert!(parallel_inflate(&bad[22..], payload.len()).is_none());
        assert!(decode(&bad, Magic::PortalSequenceData).is_err());
    }

    #[test]
    fn adler_combine_matches_direct() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 31 % 256) as u8).collect();
        for split in [0usize, 1, 5551, 5552, 70_000, 200_000] {
            let (x, y) = data.split_at(split);
            assert_eq!(adler32_combine(adler32(x), adler32(y), y.len()), adler32(&data), "split {split}");
        }
    }

    #[test]
    fn part_data_magic_is_14_bytes() {
        let env = encode(b"x", Magic::PortalPartData);
        assert_eq!(&env[..14], b"PortalPartData");
        assert_eq!(env[14..18], 1u32.to_le_bytes());
        assert!(decode(&env, Magic::PortalPartData).is_ok());
        assert!(decode(&env, Magic::PortalSequenceData).is_err());
    }

    #[test]
    fn trailing_padding_is_ignored() {
        let mut env = encode(b"data", Magic::PortalSequenceData);
        env.extend_from_slice(&[0u8; 64]);
        assert!(decode(&env, Magic::PortalSequenceData).is_ok());
    }

    #[test]
    fn errors_are_distinguished() {
        let mut env = encode(b"data", Magic::PortalSequenceData);
        assert!(matches!(decode(&env, Magic::PortalPartData), Err(EnvelopeError::BadMagic)));
        // corrupt checksum
        let n = env.len();
        env[n - 1] ^= 0xff;
        assert!(matches!(decode(&env, Magic::PortalSequenceData), Err(EnvelopeError::Zlib(_))));
        // truncated stream
        let env = encode(&b"data".repeat(1000), Magic::PortalSequenceData);
        assert!(matches!(decode(&env[..env.len() - 8], Magic::PortalSequenceData), Err(EnvelopeError::Zlib(_))));
        // declared size mismatch, both ways
        let mut env = encode(b"data", Magic::PortalSequenceData);
        env[18] = 9;
        assert_eq!(
            decode(&env, Magic::PortalSequenceData).err(),
            Some(EnvelopeError::SizeMismatch { declared: 9, actual: 4 })
        );
        env[18] = 2;
        assert!(matches!(decode(&env, Magic::PortalSequenceData), Err(EnvelopeError::SizeMismatch { declared: 2, .. })));
    }
}
