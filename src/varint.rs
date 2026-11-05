//! Unsigned protobuf-style varints, as AION 2 length prefixes and ids use them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarInt {
    pub value: i32,
    pub length: i32,
}

impl VarInt {
    pub fn invalid() -> Self {
        Self {
            value: 0,
            length: -1,
        }
    }
}

/// Read one varint at `offset`. `length` is negative when the buffer ends first
/// or the encoding runs past 32 bits.
pub fn read_varint(bytes: &[u8], offset: usize) -> VarInt {
    let mut value: i32 = 0;
    let mut shift = 0;
    let mut count = 0;

    loop {
        if offset + count >= bytes.len() {
            return VarInt::invalid();
        }
        let byte = bytes[offset + count] as u32;
        count += 1;
        value |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            return VarInt {
                value,
                length: count as i32,
            };
        }
        shift += 7;
        if shift >= 32 {
            return VarInt::invalid();
        }
    }
}

pub fn try_read_varint(bytes: &[u8], offset: &mut usize) -> Option<i32> {
    let result = read_varint(bytes, *offset);
    if result.length <= 0 {
        return None;
    }
    *offset += result.length as usize;
    if result.value < 0 {
        None
    } else {
        Some(result.value)
    }
}

pub fn encode_varint(mut value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}
