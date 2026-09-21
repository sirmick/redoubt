//! The checksum littlefs uses everywhere: CRC-32 with polynomial `0x04c11db7` (reflected:
//! `0xedb88320`), initial value `0xffffffff` and, unlike zlib's CRC-32, no final inversion.
//! A 16-entry table (one nibble at a time) keeps it small; speed does not matter here.

const TABLE: [u32; 16] = [
    0x00000000, 0x1db71064, 0x3b6e20c8, 0x26d930ac, 0x76dc4190, 0x6b6b51f4, 0x4db26158, 0x5005713c,
    0xedb88320, 0xf00f9344, 0xd6d6a3e8, 0xcb61b38c, 0x9b64c2b0, 0x86d3d2d4, 0xa00ae278, 0xbdbdf21c,
];

/// Continues `crc` over `data`. A fresh checksum starts at `0xffffffff`.
pub(crate) fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc = (crc >> 4) ^ TABLE[((crc ^ byte as u32) & 0xf) as usize];
        crc = (crc >> 4) ^ TABLE[((crc ^ (byte as u32 >> 4)) & 0xf) as usize];
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn matches_standard_crc32_before_final_inversion() {
        // The standard CRC-32 of "123456789" is 0xcbf43926; littlefs skips the final `!`.
        assert_eq!(!crc32(0xffff_ffff, b"123456789"), 0xcbf4_3926);
    }
}
