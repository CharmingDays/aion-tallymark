//! Per-direction TCP reassembly.
//!
//! Game frames do not line up with TCP segments. Segments are ordered by
//! sequence number, retransmits are dropped, and a hole realigns on the next
//! bytes that look like whole frames instead of ending the capture.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::time::Instant;

use crate::framing::{packets_in, resync_start};

const MAX_FLOWS: usize = 48;
const MAX_PENDING: usize = 32;
const MAX_GAP: i32 = 65_536;
const MAX_BUFFER: usize = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FlowKey {
    src: IpAddr,
    sport: u16,
    dst: IpAddr,
    dport: u16,
}

pub struct Assembled {
    /// Source port of the direction that carried this frame. Combat events
    /// arrive from the game server, so this becomes the locked port.
    pub src_port: u16,
    pub packet: Vec<u8>,
}

struct Flow {
    expected: Option<u32>,
    buffer: Vec<u8>,
    pending: BTreeMap<u32, Vec<u8>>,
    aligned: bool,
    seen: Instant,
}

impl Flow {
    fn new() -> Self {
        Self {
            expected: None,
            buffer: Vec::new(),
            pending: BTreeMap::new(),
            aligned: false,
            seen: Instant::now(),
        }
    }

    fn restart(&mut self, seq: u32, data: &[u8]) {
        self.buffer.clear();
        self.pending.clear();
        self.aligned = false;
        self.expected = Some(seq.wrapping_add(data.len() as u32));
        self.buffer.extend_from_slice(data);
    }

    fn push(&mut self, seq: u32, data: &[u8]) {
        self.seen = Instant::now();
        if data.is_empty() {
            return;
        }
        if self.expected.is_none() {
            self.expected = Some(seq);
        }
        let expected = self.expected.unwrap();
        let delta = (seq as i32).wrapping_sub(expected as i32);
        if delta < 0 {
            let skip = (-delta) as usize;
            if skip >= data.len() {
                return;
            }
            self.buffer.extend_from_slice(&data[skip..]);
            self.expected = Some(expected.wrapping_add((data.len() - skip) as u32));
        } else if delta == 0 {
            self.buffer.extend_from_slice(data);
            self.expected = Some(expected.wrapping_add(data.len() as u32));
        } else if delta > MAX_GAP || self.pending.len() >= MAX_PENDING {
            self.restart(seq, data);
            return;
        } else {
            self.pending.entry(seq).or_insert_with(|| data.to_vec());
            return;
        }
        self.drain_pending();
    }

    fn drain_pending(&mut self) {
        loop {
            let expected = self.expected.unwrap();
            let Some(data) = self.pending.remove(&expected) else {
                break;
            };
            self.buffer.extend_from_slice(&data);
            self.expected = Some(expected.wrapping_add(data.len() as u32));
        }
    }

    fn take_packets(&mut self) -> Vec<Vec<u8>> {
        if !self.aligned {
            match resync_start(&self.buffer) {
                Some(0) => self.aligned = true,
                Some(start) => {
                    self.buffer.drain(..start);
                    self.aligned = true;
                }
                None => {
                    if self.buffer.len() > MAX_BUFFER {
                        let keep = crate::framing::RESYNC_SEARCH.min(self.buffer.len());
                        self.buffer.drain(..self.buffer.len() - keep);
                    }
                    return Vec::new();
                }
            }
        }
        let (packets, consumed) = packets_in(&self.buffer);
        if consumed == 0 && self.buffer.len() > MAX_BUFFER {
            self.aligned = false;
            return Vec::new();
        }
        self.buffer.drain(..consumed);
        packets
    }
}

pub struct Assembler {
    flows: HashMap<FlowKey, Flow>,
    locked_port: Option<u16>,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
            locked_port: None,
        }
    }

    pub fn locked_port(&self) -> Option<u16> {
        self.locked_port
    }

    pub fn lock_port(&mut self, port: u16) {
        if self.locked_port.is_none() {
            self.locked_port = Some(port);
            self.flows
                .retain(|key, _| key.sport == port || key.dport == port);
        }
    }

    pub fn push(
        &mut self,
        src: IpAddr,
        sport: u16,
        dst: IpAddr,
        dport: u16,
        seq: u32,
        payload: &[u8],
    ) -> Vec<Assembled> {
        if payload.is_empty() {
            return Vec::new();
        }
        if let Some(port) = self.locked_port {
            if sport != port && dport != port {
                return Vec::new();
            }
        }
        if self.flows.len() >= MAX_FLOWS
            && !self.flows.contains_key(&FlowKey {
                src,
                sport,
                dst,
                dport,
            })
        {
            if let Some(stale) = self
                .flows
                .iter()
                .min_by_key(|(_, flow)| flow.seen)
                .map(|(key, _)| *key)
            {
                self.flows.remove(&stale);
            }
        }
        let key = FlowKey {
            src,
            sport,
            dst,
            dport,
        };
        let flow = self.flows.entry(key).or_insert_with(Flow::new);
        flow.push(seq, payload);
        flow.take_packets()
            .into_iter()
            .map(|packet| Assembled {
                src_port: sport,
                packet,
            })
            .collect()
    }
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::encode_frame;
    use std::net::Ipv4Addr;

    fn body(marker: u8) -> Vec<u8> {
        vec![0x04, 0x38, marker, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60]
    }

    #[test]
    fn segments_reassemble_before_frames_are_cut() {
        let stream = (1..=3)
            .flat_map(|marker| encode_frame(&body(marker)))
            .collect::<Vec<_>>();
        let split = stream.len() / 2;
        let mut assembler = Assembler::new();
        let src = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let dst = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let first = assembler.push(src, 2100, dst, 50000, 1000, &stream[..split]);
        assert!(
            first.is_empty(),
            "a frame cut by the segment is not parsed yet"
        );
        let second = assembler.push(src, 2100, dst, 50000, 1000 + split as u32, &stream[split..]);
        assert_eq!(second.len(), 3);
        assert_eq!(second[0].src_port, 2100);
    }

    #[test]
    fn a_retransmit_is_not_parsed_twice() {
        let stream = (1..=3)
            .flat_map(|marker| encode_frame(&body(marker)))
            .collect::<Vec<_>>();
        let mut assembler = Assembler::new();
        let src = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let dst = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let once = assembler.push(src, 2100, dst, 50000, 50, &stream);
        let twice = assembler.push(src, 2100, dst, 50000, 50, &stream);
        assert_eq!(once.len(), 3);
        assert!(twice.is_empty());
    }
}
