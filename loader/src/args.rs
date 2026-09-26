//! Builds the tagged argument block the kernel reads at boot.
//!
//! Framing is the same as rv32 Redoubt: `tag: u32, crc16: u16, words: u16, data: [u32]`.
//! Payloads that carry addresses are 64-bit clean: `XArg` is version 2 and `MREx`
//! entries are `{ start: u64, size: u64, tag: u32, pad: u32 }`.

use crc::{Crc, CRC_16_IBM_SDLC};

/// Same polynomial `create-image` uses (`crc16::X25`).
const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_SDLC);

pub struct ArgsBuilder {
    buf: &'static mut [u32],
    len: usize,
    tag_start: usize,
}

impl ArgsBuilder {
    /// `buf` must start with room for the `XArg` tag, which `finish()` fills in.
    pub fn new(buf: &'static mut [u32]) -> Self {
        let mut builder = ArgsBuilder { buf, len: 0, tag_start: 0 };
        builder.begin(b"XArg");
        for _ in 0..XARG_WORDS {
            builder.word(0);
        }
        builder.end();
        builder
    }

    pub fn begin(&mut self, name: &[u8; 4]) {
        self.tag_start = self.len;
        self.word(u32::from_le_bytes(*name));
        self.word(0);
    }

    pub fn word(&mut self, value: u32) {
        // The block is a fixed number of pages (`ARGS_PAGES`), which a bundle with many
        // processes, names and devices can fill. Say so rather than panicking on the index.
        assert!(self.len < self.buf.len(), "argument block full");
        self.buf[self.len] = value;
        self.len += 1;
    }

    pub fn word64(&mut self, value: u64) {
        self.word(value as u32);
        self.word((value >> 32) as u32);
    }

    /// Append bytes, zero-padded to a whole number of words.
    pub fn bytes(&mut self, data: &[u8]) {
        for chunk in data.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            self.word(u32::from_le_bytes(word));
        }
    }

    pub fn end(&mut self) {
        let data = &self.buf[self.tag_start + 2..self.len];
        let words = data.len() as u32;
        let mut digest = CRC.digest();
        for word in data {
            digest.update(&word.to_le_bytes());
        }
        self.buf[self.tag_start + 1] = digest.finalize() as u32 | words << 16;
    }

    /// Fill in `XArg` now that the total size is known.
    pub fn finish(mut self, ram_start: usize, ram_size: usize, ram_name: &[u8; 4]) {
        let total_words = self.len as u32;
        self.len = 0;
        self.begin(b"XArg");
        self.word(total_words);
        self.word(2);
        self.word64(ram_start as u64);
        self.word64(ram_size as u64);
        self.word(u32::from_le_bytes(*ram_name));
        self.end();
    }
}

const XARG_WORDS: usize = 7;
