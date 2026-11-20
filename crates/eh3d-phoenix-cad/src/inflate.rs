// SPDX-License-Identifier: GPL-3.0-or-later
//! The reader of DEFLATE data (RFC 1951), with no dependency: the compression that the packages of
//! 3MF files, which other programs write, use for their entries.
//!
//! The reader never panics on damaged data and never writes more than the limit it is given, so a
//! small file that claims to hold an enormous one is refused instead of filling the memory.

/// How many bits a code of the tables can have.
const MAX_BITS: usize = 15;

/// The base length of each length symbol (257 to 285).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// The extra bits of each length symbol.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// The base distance of each distance symbol.
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// The extra bits of each distance symbol.
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order in which the lengths of the codes of the lengths are stored.
const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The reader of bits, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    buffer: u32,
    count: u32,
}

impl Bits<'_> {
    fn take(&mut self, need: u32) -> Result<u32, String> {
        let mut value = self.buffer;
        while self.count < need {
            let byte = *self
                .data
                .get(self.at)
                .ok_or_else(|| "the compressed data ends too soon".to_owned())?;
            self.at += 1;
            value |= u32::from(byte) << self.count;
            self.count += 8;
        }
        self.buffer = value >> need;
        self.count -= need;
        Ok(value & ((1u32 << need) - 1))
    }
}

/// A canonical Huffman code: how many codes there are of each length and the symbols in order.
struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// The code of the lengths given; refused when the lengths over-subscribe the code.
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut count = [0u16; MAX_BITS + 1];
        for len in lengths {
            count[usize::from(*len)] += 1;
        }
        let mut left: i32 = 1;
        for in_length in &count[1..] {
            left <<= 1;
            left -= i32::from(*in_length);
            if left < 0 {
                return Err("a table of codes is over-subscribed".to_owned());
            }
        }
        let mut offset = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            offset[len + 1] = offset[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, len) in lengths.iter().enumerate() {
            if *len != 0 {
                symbol[usize::from(offset[usize::from(*len)])] = sym as u16;
                offset[usize::from(*len)] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<usize, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..=MAX_BITS {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return self
                    .symbol
                    .get((index + (code - first)) as usize)
                    .map(|s| usize::from(*s))
                    .ok_or_else(|| "a code is not in the table".to_owned());
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("a code is not in the table".to_owned())
    }
}

/// The codes of the fixed blocks.
fn fixed() -> Result<(Huffman, Huffman), String> {
    let mut lengths = [0u8; 288];
    for (sym, len) in lengths.iter_mut().enumerate() {
        *len = match sym {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 30])?))
}

/// The codes of a block with its own tables.
fn dynamic(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), String> {
    let nlen = bits.take(5)? as usize + 257;
    let ndist = bits.take(5)? as usize + 1;
    let ncode = bits.take(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err("a block has too many codes".to_owned());
    }
    let mut lengths = [0u8; 320];
    for slot in CODE_ORDER.iter().take(ncode) {
        lengths[*slot] = bits.take(3)? as u8;
    }
    let code_lengths = Huffman::new(&lengths[..19])?;
    let mut lengths = [0u8; 320];
    let mut index = 0;
    while index < nlen + ndist {
        let symbol = code_lengths.decode(bits)?;
        if symbol < 16 {
            lengths[index] = symbol as u8;
            index += 1;
        } else {
            let (value, repeat) = match symbol {
                16 => {
                    if index == 0 {
                        return Err("a block repeats a length that is not there".to_owned());
                    }
                    (lengths[index - 1], 3 + bits.take(2)? as usize)
                }
                17 => (0, 3 + bits.take(3)? as usize),
                _ => (0, 11 + bits.take(7)? as usize),
            };
            if index + repeat > nlen + ndist {
                return Err("a block repeats more lengths than it has".to_owned());
            }
            for slot in &mut lengths[index..index + repeat] {
                *slot = value;
            }
            index += repeat;
        }
    }
    if lengths[256] == 0 {
        return Err("a block has no end code".to_owned());
    }
    Ok((
        Huffman::new(&lengths[..nlen])?,
        Huffman::new(&lengths[nlen..nlen + ndist])?,
    ))
}

/// The data that a DEFLATE stream holds, refused when it is damaged or holds more than `limit`
/// bytes.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits {
        data,
        at: 0,
        buffer: 0,
        count: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    let too_big = || format!("the data holds more than {limit} bytes");
    loop {
        let last = bits.take(1)?;
        match bits.take(2)? {
            0 => {
                bits.buffer = 0;
                bits.count = 0;
                let head = data
                    .get(bits.at..bits.at + 4)
                    .ok_or_else(|| "the compressed data ends too soon".to_owned())?;
                let len = usize::from(u16::from_le_bytes([head[0], head[1]]));
                let check = u16::from_le_bytes([head[2], head[3]]);
                if check != !(len as u16) {
                    return Err("a stored block has a length that does not check".to_owned());
                }
                bits.at += 4;
                let block = data
                    .get(bits.at..bits.at + len)
                    .ok_or_else(|| "the compressed data ends too soon".to_owned())?;
                if out.len() + len > limit {
                    return Err(too_big());
                }
                out.extend_from_slice(block);
                bits.at += len;
            }
            kind @ (1 | 2) => {
                let (lit, dist) = if kind == 1 {
                    fixed()?
                } else {
                    dynamic(&mut bits)?
                };
                loop {
                    let symbol = lit.decode(&mut bits)?;
                    if symbol < 256 {
                        if out.len() >= limit {
                            return Err(too_big());
                        }
                        out.push(symbol as u8);
                    } else if symbol == 256 {
                        break;
                    } else {
                        let symbol = symbol - 257;
                        if symbol >= 29 {
                            return Err("a length code is not valid".to_owned());
                        }
                        let len = usize::from(LENGTH_BASE[symbol])
                            + bits.take(u32::from(LENGTH_EXTRA[symbol]))? as usize;
                        let symbol = dist.decode(&mut bits)?;
                        if symbol >= 30 {
                            return Err("a distance code is not valid".to_owned());
                        }
                        let back = usize::from(DISTANCE_BASE[symbol])
                            + bits.take(u32::from(DISTANCE_EXTRA[symbol]))? as usize;
                        if back > out.len() {
                            return Err("a copy reaches back before the data".to_owned());
                        }
                        if out.len() + len > limit {
                            return Err(too_big());
                        }
                        let from = out.len() - back;
                        for k in 0..len {
                            let byte = out[from + k];
                            out.push(byte);
                        }
                    }
                }
            }
            _ => return Err("a block is of a kind that does not exist".to_owned()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// A DEFLATE stream made of stored blocks of at most 65535 bytes, for the tests and for the
/// programs that need a valid stream without compressing.
pub fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5 * (data.len() / 65_535 + 1));
    let mut chunks = data.chunks(65_535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
        return out;
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_block_with_a_copy_is_read() {
        // "aaaaaaaaaa" compressed by zlib at the best level, without the zlib head and tail
        let data = [0x4b, 0x4c, 0x84, 0x01, 0x00];
        assert_eq!(inflate(&data, 100).unwrap(), b"aaaaaaaaaa");
    }

    #[test]
    fn a_dynamic_block_is_read() {
        // 30 lines of text compressed by zlib at the best level, with codes of their own
        let packed: [u8; 265] = [
            0x75, 0x93, 0x4b, 0x6a, 0x03, 0x31, 0x10, 0x05, 0xf7, 0x3e, 0x85, 0x4e, 0x60, 0xa6,
            0x5b, 0xad, 0x9f, 0xef, 0xa3, 0xe0, 0x80, 0xed, 0x40, 0x62, 0x82, 0x8f, 0x6f, 0x35,
            0xa3, 0x37, 0x68, 0xf1, 0xbc, 0x90, 0x56, 0xc5, 0x30, 0x45, 0x3d, 0xdd, 0xbe, 0x1f,
            0x3d, 0x6c, 0xe1, 0xe7, 0x2b, 0x3c, 0xaf, 0x3d, 0xdc, 0xfb, 0xdf, 0xf5, 0x12, 0xfe,
            0xfb, 0xef, 0xb3, 0xbf, 0xc2, 0x76, 0x4e, 0xe3, 0xe8, 0xb8, 0x4e, 0x37, 0xa7, 0x84,
            0x52, 0x71, 0x50, 0xc5, 0x29, 0xd9, 0x29, 0xa5, 0x54, 0x1e, 0x94, 0x38, 0xa5, 0x3b,
            0x15, 0x29, 0xd5, 0x06, 0x55, 0x9d, 0x8a, 0x3b, 0x65, 0x94, 0x12, 0x1d, 0x98, 0x3a,
            0x66, 0x3b, 0x96, 0x38, 0x96, 0x06, 0xd6, 0x96, 0xff, 0xcf, 0x1c, 0xab, 0x03, 0x8b,
            0x8b, 0x40, 0xa1, 0x98, 0x8a, 0x1b, 0x6c, 0x8b, 0x42, 0xe5, 0x9c, 0x0d, 0xce, 0x16,
            0x87, 0xc6, 0xb1, 0xe2, 0x9f, 0x93, 0x45, 0x42, 0x78, 0x84, 0xe8, 0x15, 0xd2, 0x5a,
            0xe1, 0x43, 0x06, 0xef, 0x20, 0xba, 0x78, 0x08, 0x2f, 0x11, 0x3d, 0x45, 0x5e, 0x3c,
            0x84, 0xb7, 0x88, 0x0d, 0xf9, 0xa7, 0x88, 0xf0, 0x1a, 0xa6, 0x18, 0x00, 0x44, 0x78,
            0x0e, 0x4b, 0x98, 0x00, 0x44, 0x78, 0x0f, 0xab, 0x18, 0x01, 0x3c, 0x78, 0x90, 0x24,
            0x58, 0x01, 0x3c, 0x78, 0x90, 0x64, 0x98, 0x01, 0x3c, 0x78, 0x91, 0x54, 0xb0, 0x83,
            0xe9, 0xa1, 0x3c, 0x48, 0xde, 0x8e, 0x21, 0x4c, 0x11, 0xe5, 0x45, 0x72, 0xc4, 0x12,
            0xf0, 0x32, 0x3e, 0x3c, 0x8d, 0x7c, 0x4c, 0x61, 0x9a, 0x28, 0x2f, 0x92, 0x1b, 0xa6,
            0x30, 0x4d, 0x94, 0x17, 0x29, 0x7a, 0x4c, 0x01, 0x2a, 0x3c, 0x49, 0x49, 0x98, 0x02,
            0x4c, 0x78, 0x92, 0x52, 0x31, 0x05, 0x98, 0xf0, 0x24, 0x55, 0x30, 0x05, 0x88, 0xf0,
            0x24, 0xd5, 0x30, 0x05, 0x88, 0xf0, 0x24, 0xb5, 0x60, 0x0a, 0x76, 0x7a, 0x03,
        ];
        let text = inflate(&packed, 10_000).unwrap();
        assert_eq!(text.len(), 1192);
        let text = String::from_utf8(text).unwrap();
        assert!(text.starts_with(
            "line 0 of the mesh: vertex 0.5 0.25 0
"
        ));
        assert!(text.ends_with(
            "line 29 of the mesh: vertex 87.5 8.25 4
"
        ));
    }

    #[test]
    fn stored_blocks_round_trip_for_any_length() {
        for len in [0usize, 1, 5, 65_534, 65_535, 65_536, 200_000] {
            let data: Vec<u8> = (0..len).map(|k| (k * 7 % 251) as u8).collect();
            let packed = deflate_stored(&data);
            assert_eq!(inflate(&packed, len).unwrap(), data, "{len}");
        }
    }

    #[test]
    fn data_over_the_limit_is_refused() {
        let packed = deflate_stored(&[1u8; 1000]);
        assert!(inflate(&packed, 999).unwrap_err().contains("more than 999"));
        assert!(inflate(&packed, 1000).is_ok());
        // a copy that would pass the limit is refused too
        let data = [0x4b, 0x4c, 0x84, 0x01, 0x00];
        assert!(inflate(&data, 9).is_err());
    }

    #[test]
    fn damaged_streams_are_refused_and_never_panic() {
        assert!(inflate(&[], 10).is_err());
        // a block of the kind 3
        assert!(inflate(&[0x07], 10).is_err());
        // a stored block whose length does not check
        assert!(inflate(&[0x01, 0x05, 0x00, 0x00, 0x00], 10).is_err());
        // a copy before the data (fixed block, distance 1 with nothing written)
        assert!(inflate(&[0x03, 0x02], 10).is_err());
        // every prefix and every flipped byte of a valid stream, with a fixed seed
        let mut seed = 0x1234_5678_u32;
        let data = [0x4b, 0x4c, 0x84, 0x01, 0x00];
        for cut in 0..data.len() {
            let _ = inflate(&data[..cut], 100);
        }
        for _ in 0..2000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let len = 1 + (seed >> 24) as usize % 40;
            let junk: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 16) as u8
                })
                .collect();
            let _ = inflate(&junk, 1000);
        }
    }

    #[test]
    fn a_table_of_codes_that_is_over_subscribed_is_refused() {
        assert!(Huffman::new(&[1, 1, 1]).is_err());
        assert!(Huffman::new(&[1, 1]).is_ok());
    }
}
