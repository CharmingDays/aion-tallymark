//! Split a reassembled TCP byte stream into game frames.
//!
//! Wire shape, as understood by the A2Tools meter this layout is derived from:
//!
//! ```text
//! 00                      padding, skipped
//! 17 03 03 ..             a TLS record from another connection, skipped
//! <varint len> <body>     a packet; len counts the body plus 4
//! FF FF <u32 le> <lz4>    a compressed bundle of further packets
//! ```
//!
//! A frame occupies `len - 4 + varint_width` bytes. Capture starts mid-stream,
//! so a length that cannot be a frame is skipped one byte and the walk continues.

use crate::varint::{encode_varint, read_varint};

const MAX_PACKET_BYTES: usize = 65_535;
const MAX_FRAGMENT_WAIT_BYTES: usize = 16_384;
const MAX_DECOMPRESSED_BYTES: usize = 1_000_000;
pub const MAX_BUNDLE_DEPTH: usize = 4;
pub const RESYNC_SEARCH: usize = 8192;

/// Second byte of almost every opcode: `xx 36/37/38`, `00 8D`, or a bundle's `FF`.
const COMMON_OPCODE_HIGH: [u8; 5] = [0x36, 0x37, 0x38, 0x8D, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Packet,
    Bundle,
}

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub kind: FrameKind,
    pub start: usize,
    pub end: usize,
    pub payload_start: usize,
}

impl Frame {
    pub fn bytes<'a>(&self, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[self.start..self.end]
    }

    pub fn payload<'a>(&self, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[self.start + self.payload_start..self.end]
    }
}

#[derive(Debug, Default)]
pub struct Framing {
    pub frames: Vec<Frame>,
    /// Bytes that formed whole frames or were skipped. Keep the rest for the next read.
    pub consumed: usize,
}

/// Bytes a frame occupies. `None` when the length is shorter than its own varint.
pub fn frame_size(value: i32, varint_bytes: i32) -> Option<usize> {
    let size = value as i64 - 4 + varint_bytes as i64;
    (size >= varint_bytes as i64 && size > 0).then_some(size as usize)
}

/// Length-varint value for a body of `body_len` bytes (the bytes after the varint).
pub fn length_value(body_len: usize) -> u32 {
    body_len as u32 + 4
}

pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let mut out = encode_varint(length_value(body.len()));
    out.extend_from_slice(body);
    out
}

/// Walk `buffer`, stopping at the first frame that still needs more bytes.
pub fn walk(buffer: &[u8]) -> Framing {
    walk_with(buffer, true)
}

/// Walk a buffer the game itself framed (a decompressed bundle). A bad length
/// stops the walk instead of inventing packets one byte later.
pub fn walk_inner(buffer: &[u8]) -> Framing {
    walk_with(buffer, false)
}

fn walk_with(buffer: &[u8], resync: bool) -> Framing {
    let mut out = Framing::default();
    let mut offset = 0usize;

    while offset < buffer.len() {
        if buffer[offset] == 0x00 {
            offset += 1;
            continue;
        }

        if resync {
            if let Some(size) = tls_record_size(&buffer[offset..]) {
                if offset + size > buffer.len() {
                    break;
                }
                offset += size;
                continue;
            }
        }

        let length = read_varint(buffer, offset);
        if length.length <= 0 || length.value <= 0 {
            if !resync || offset + 5 > buffer.len() {
                break;
            }
            offset += 1;
            continue;
        }

        let Some(total) = frame_size(length.value, length.length) else {
            if !resync {
                break;
            }
            offset += 1;
            continue;
        };
        if total > MAX_PACKET_BYTES {
            if !resync {
                break;
            }
            offset += 1;
            continue;
        }

        if offset + total > buffer.len() {
            let payload_at = offset + length.length as usize;
            let limit = if bundle_header_at(buffer, payload_at) {
                MAX_PACKET_BYTES
            } else {
                MAX_FRAGMENT_WAIT_BYTES
            };
            if total > limit {
                if !resync {
                    break;
                }
                offset += 1;
                continue;
            }
            break;
        }

        let payload_start = length.length as usize;
        let is_bundle = bundle_header_at(buffer, offset + payload_start);
        out.frames.push(Frame {
            kind: if is_bundle {
                FrameKind::Bundle
            } else {
                FrameKind::Packet
            },
            start: offset,
            end: offset + total,
            payload_start,
        });
        offset += total;
    }

    out.consumed = offset;
    out
}

/// First offset at which the following bytes look like whole frames.
///
/// Used after a capture hole: the next segment continues a frame that was cut,
/// so its first byte is not a length. `None` means wait for more bytes.
pub fn resync_start(buffer: &[u8]) -> Option<usize> {
    let starts_here = |from: usize| -> bool {
        let mut at = from;
        let mut whole = 0;
        let mut first_is_bundle = false;
        loop {
            while at < buffer.len() && buffer[at] == 0x00 {
                at += 1;
            }
            if at >= buffer.len() {
                return whole >= 2 || (whole == 1 && first_is_bundle);
            }
            let length = read_varint(buffer, at);
            if length.length <= 0 || length.value <= 0 {
                return false;
            }
            let Some(size) = frame_size(length.value, length.length) else {
                return false;
            };
            if size > MAX_PACKET_BYTES {
                return false;
            }
            let payload = at + length.length as usize;
            let opens = buffer
                .get(payload + 1)
                .is_some_and(|b| COMMON_OPCODE_HIGH.contains(b));
            if whole < 2 && !opens {
                return false;
            }
            let bundle = bundle_header_at(buffer, payload);
            if at + size > buffer.len() {
                let limit = if bundle {
                    MAX_PACKET_BYTES
                } else {
                    MAX_FRAGMENT_WAIT_BYTES
                };
                return size <= limit && (whole >= 2 || (whole == 1 && bundle));
            }
            if whole == 0 {
                first_is_bundle = bundle;
            }
            whole += 1;
            if whole >= 3 {
                return true;
            }
            at += size;
        }
    };

    (0..buffer.len().min(RESYNC_SEARCH)).find(|&from| buffer[from] != 0x00 && starts_here(from))
}

fn bundle_header_at(buffer: &[u8], at: usize) -> bool {
    let Some(header) = buffer.get(at..at + 6) else {
        return false;
    };
    let size = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
    header[0] == 0xFF && header[1] == 0xFF && size > 0 && size <= MAX_DECOMPRESSED_BYTES
}

/// Bytes a TLS record occupies, header included. Content type 20–23, TLS 1.0–1.3.
fn tls_record_size(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 5
        || !(0x14..=0x17).contains(&bytes[0])
        || bytes[1] != 0x03
        || !(0x01..=0x04).contains(&bytes[2])
    {
        return None;
    }
    let len = u16::from_be_bytes([bytes[3], bytes[4]]) as usize;
    (len > 0 && len <= 16_384 + 256).then_some(5 + len)
}

/// Decompress a bundle payload that starts at `FF FF`.
pub fn decompress_bundle(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 7 {
        return None;
    }
    let size = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
    if size == 0 || size > MAX_DECOMPRESSED_BYTES {
        return None;
    }
    // lz4_flex's prepended-size helper wants the size in front of the block.
    // The game stores that size itself, so feed the block and the known size.
    lz4_flex::decompress(&payload[6..], size).ok()
}

/// Expand `root` (a framed buffer) into packet frames, opening bundles up to
/// [`MAX_BUNDLE_DEPTH`].
pub fn packets_in(buffer: &[u8]) -> (Vec<Vec<u8>>, usize) {
    let walked = walk(buffer);
    let mut packets = Vec::new();
    for frame in &walked.frames {
        match frame.kind {
            FrameKind::Packet => packets.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => collect_bundle(frame.payload(buffer), 1, &mut packets),
        }
    }
    (packets, walked.consumed)
}

fn collect_bundle(payload: &[u8], depth: usize, out: &mut Vec<Vec<u8>>) {
    if depth > MAX_BUNDLE_DEPTH {
        return;
    }
    let Some(plain) = decompress_bundle(payload) else {
        return;
    };
    let walked = walk_inner(&plain);
    for frame in &walked.frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(&plain).to_vec()),
            FrameKind::Bundle => collect_bundle(frame.payload(&plain), depth + 1, out),
        }
    }
}

/// Build a bundle frame whose inner contents are already framed packets.
pub fn encode_bundle(inner_framed: &[u8]) -> Vec<u8> {
    let compressed = lz4_flex::compress(inner_framed);
    let mut body = vec![0xFF, 0xFF];
    body.extend_from_slice(&(inner_framed.len() as u32).to_le_bytes());
    body.extend_from_slice(&compressed);
    encode_frame(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn damage_body(marker: u8) -> Vec<u8> {
        let mut body = vec![0x04, 0x38, marker, 0x01, 0x02, 0x03, 0x04, 0x05];
        body.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        body
    }

    #[test]
    fn a_packet_roundtrips_and_padding_is_skipped() {
        let frame = encode_frame(&damage_body(1));
        let mut buffer = vec![0x00, 0x00];
        buffer.extend_from_slice(&frame);
        let walked = walk(&buffer);
        assert_eq!(walked.frames.len(), 1);
        assert_eq!(walked.frames[0].kind, FrameKind::Packet);
        assert_eq!(walked.consumed, buffer.len());
        assert_eq!(walked.frames[0].bytes(&buffer), frame.as_slice());
    }

    #[test]
    fn a_tls_record_is_skipped() {
        let mut buffer = vec![0x17, 0x03, 0x03, 0x00, 0x05, 1, 2, 3, 4, 5];
        let frame = encode_frame(&damage_body(2));
        buffer.extend_from_slice(&frame);
        let walked = walk(&buffer);
        assert_eq!(walked.frames.len(), 1);
        assert_eq!(walked.frames[0].bytes(&buffer), frame.as_slice());
    }

    #[test]
    fn an_incomplete_frame_is_kept() {
        let frame = encode_frame(&damage_body(3));
        let walked = walk(&frame[..frame.len() - 3]);
        assert!(walked.frames.is_empty());
        assert_eq!(walked.consumed, 0);
    }

    #[test]
    fn a_bundle_opens_into_its_packets() {
        let inner = encode_frame(&damage_body(4));
        let bundle = encode_bundle(&inner);
        let (packets, consumed) = packets_in(&bundle);
        assert_eq!(consumed, bundle.len());
        assert_eq!(packets, vec![inner]);
    }

    #[test]
    fn resync_finds_three_frames_after_garbage() {
        let stream = [damage_body(1), damage_body(2), damage_body(3)]
            .into_iter()
            .flat_map(|body| encode_frame(&body))
            .collect::<Vec<_>>();
        let mut buffer = vec![0xB0, 0x0C, 0x37, 0x92, 0x11, 0x22];
        buffer.extend_from_slice(&stream);
        let start = resync_start(&buffer).expect("frame start");
        assert_eq!(start, 6);
        let walked = walk(&buffer[start..]);
        assert_eq!(walked.frames.len(), 3);
    }
}
