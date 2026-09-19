//! SHA-256 (FIPS 180-4), for the SSH exchange hash of RFC 4253 §8 (`curve25519-sha256`,
//! RFC 8731). That hash is the one place `keyd` needs a hash it does not get from
//! `ed25519-compact` (which brings SHA-512 for Ed25519 itself).
//!
//! Written here rather than taken from a crate (TENETS.md 5: reuse when the crate is small,
//! `no_std`, pure Rust and we have read it, "otherwise we write the 50 lines"). The maintained
//! option, `sha2`, is fine code but arrives with `digest`, `block-buffer`, `crypto-common`,
//! `generic-array`/`typenum`, `cfg-if` and `cpufeatures` behind it, which is six more crates
//! inside the process that holds every private key on the box. This is one function with one
//! constant table and no dependencies.
//!
//! **Constant time.** There is no branch and no memory index that depends on the *contents* of
//! the input: the compression function is straight-line 32-bit arithmetic over a fixed
//! 64-round loop, and the only branches are on how many bytes are buffered, which is a length.
//! Lengths here are public (`keyd` replies with a fixed-size signature whatever the input).

/// The 64 round constants: the first 32 bits of the fractional parts of the cube roots of the
/// first 64 primes (FIPS 180-4, §4.2.2).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The initial hash value: the first 32 bits of the fractional parts of the square roots of the
/// first eight primes (FIPS 180-4, §5.3.3).
const INIT: [u32; 8] =
    [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

/// The block size, in bytes.
const BLOCK: usize = 64;

/// The digest, in bytes.
pub const DIGEST: usize = 32;

/// A SHA-256 in progress. `update` as often as you like, then `finish`.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    /// Bytes of the current block that are filled.
    filled: usize,
    block: [u8; BLOCK],
    /// The message's length so far, in bytes. A message longer than 2^61 bytes cannot be fed
    /// through a 64 KiB lend in this lifetime, so saturating here is unreachable, not a limit.
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Sha256 { Sha256::new() }
}

impl Sha256 {
    pub const fn new() -> Sha256 { Sha256 { state: INIT, filled: 0, block: [0; BLOCK], len: 0 } }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let room = BLOCK - self.filled;
            let take = room.min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == BLOCK {
                let block = self.block;
                compress(&mut self.state, &block);
                self.filled = 0;
            }
        }
    }

    /// The digest of everything fed in (FIPS 180-4 §5.1.1 padding).
    pub fn finish(mut self) -> [u8; DIGEST] {
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        // Pad with zeros until 8 bytes are left in the block; `filled` is never 64 here,
        // because a full block is compressed at once.
        let zeros = (BLOCK + BLOCK - 8 - self.filled) % BLOCK;
        self.update(&[0; BLOCK][..zeros]);
        self.update(&bits.to_be_bytes());
        debug_assert_eq!(self.filled, 0);
        let mut out = [0; DIGEST];
        for (chunk, word) in out.chunks_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// The digest of one slice.
pub fn hash(data: &[u8]) -> [u8; DIGEST] {
    let mut h = Sha256::new();
    h.update(data);
    h.finish()
}

/// One block (FIPS 180-4, §6.2.2). Straight-line: the loops are fixed and every index is a
/// constant of the round, never a function of the data.
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK]) {
    let mut w = [0u32; 64];
    for (word, bytes) in w[..16].iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn hex(bytes: &[u8]) -> alloc::string::String {
        use core::fmt::Write as _;
        let mut s = alloc::string::String::new();
        for b in bytes {
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    /// FIPS 180-4's own examples, plus the two the NIST byte-oriented test vectors start with.
    #[test]
    fn fips_180_4_vectors() {
        let cases: [(&[u8], &str); 4] = [
            (b"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
        ];
        for (message, want) in cases {
            assert_eq!(hex(&hash(message)), want, "{:?}", core::str::from_utf8(message));
        }
        // One million 'a': the classic long vector.
        let mut h = Sha256::new();
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(hex(&h.finish()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    /// Every way of splitting a message into chunks gives the same digest, so the buffering
    /// (the one place `filled` is a branch) is right at every boundary.
    #[test]
    fn chunking_never_changes_the_digest() {
        let message: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        for len in 0..=message.len() {
            let want = hash(&message[..len]);
            for chunk in [1, 3, 31, 63, 64, 65, 127, 128] {
                let mut h = Sha256::new();
                for part in message[..len].chunks(chunk) {
                    h.update(part);
                }
                assert_eq!(h.finish(), want, "len {len} in chunks of {chunk}");
            }
        }
        // Exactly one block, and one byte short of needing a second padding block.
        for len in [55, 56, 57, 63, 64, 119, 120] {
            let m = vec![0x5au8; len];
            let mut h = Sha256::new();
            for part in m.chunks(1) {
                h.update(part);
            }
            assert_eq!(h.finish(), hash(&m), "len {len}");
        }
    }
}
