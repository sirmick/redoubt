//! CTZ skip-lists: how file data is laid out (DESIGN.md "CTZ skip-lists").
//!
//! A file is a backwards linked list of blocks. Block `n` (counting from the file's start)
//! begins with `ctz(n) + 1` pointers: to blocks `n - 1`, `n - 2`, `n - 4`, ... `n - 2^ctz(n)`.
//! Block 0 has none. The rest of each block is data. The file's struct names the last block
//! (the head) and the file size, which is enough to find everything.
//!
//! Only the arithmetic lives here; walking the list needs the device (`crate::fs`).

/// Where byte `pos` of a file lives: `(block index, offset within that block)`.
///
/// The reference's `lfs_ctz_index`: block `i` holds `block_size - 4 * (ctz(i) + 1)` bytes of
/// data (block 0: all of it), and the sum of `ctz(i) + 1` over `0..i` is `2i - popcount(i)`,
/// which gives a closed form. Computed in `u64` so no input can overflow.
pub(crate) fn index(block_size: u32, pos: u32) -> (u32, u32) {
    let b = block_size as u64 - 2 * 4;
    let pos = pos as u64;
    let i = pos / b;
    if i == 0 {
        return (0, pos as u32);
    }
    let i = (pos - 4 * ((i - 1).count_ones() as u64 + 2)) / b;
    let off = pos - b * i - 4 * i.count_ones() as u64;
    (i as u32, off as u32)
}

/// How many pointers block `index` begins with.
pub(crate) fn skips(index: u32) -> u32 { index.trailing_zeros() + 1 }

/// On the way from block `current` down to block `target`, which pointer of `current` to
/// follow: the longest jump that does not overshoot.
pub(crate) fn jump(current: u32, target: u32) -> u32 {
    let distance = current - target + 1; // >= 2
    let npw2 = 32 - (distance - 1).leading_zeros(); // ceil(log2(distance))
    (npw2 - 1).min(current.trailing_zeros())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks the layout block by block and checks `index` against it.
    #[test]
    fn index_matches_the_layout() {
        for bs in [128u32, 256, 512, 4096] {
            let mut pos = 0u32;
            for i in 0..3000u32 {
                let header = if i == 0 { 0 } else { 4 * skips(i) };
                for off in [header, header + 1, bs - 1] {
                    let p = pos + (off - header);
                    assert_eq!(index(bs, p), (i, off), "bs {bs} block {i} off {off}");
                }
                pos += bs - header;
            }
        }
    }

    #[test]
    fn jumps_never_overshoot() {
        for current in 1..2000u32 {
            for target in 0..current {
                let s = jump(current, target);
                assert!(s < skips(current));
                assert!(current - (1 << s) >= target);
            }
        }
    }
}
