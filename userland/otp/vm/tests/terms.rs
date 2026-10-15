//! Term comparison (docs/userland/beamlet.md, "Limits inside one VM"): a term is equal to itself
//! without being read.

use std::cmp::Ordering;
use std::time::{Duration, Instant};

use beamlet_vm::term::{Heap, Literals};

/// `erl_eval` compares every binding with itself on each call of an interpreted fun, so a
/// 256 KiB binary at the prompt once cost a scan a step. Ten thousand comparisons of a 64 MiB
/// binary with itself would read 640 GB; they must finish in seconds. Two sub-binaries over one
/// window are the same bits too; equal bytes elsewhere still compare equal, and different bytes
/// still differ.
#[test]
fn a_term_equal_to_itself_is_not_scanned() {
    let mut h = Heap::new(&Literals::default());
    let bytes = vec![0x5au8; 64 << 20];
    let big = h.binary(&bytes);
    let window = h.as_bits(big).expect("bits").slice(8, 64 * 8);
    let (one, two) = (h.bits(window.clone()), h.bits(window));
    let started = Instant::now();
    for _ in 0..10_000 {
        assert!(h.eq_exact(big, big));
        assert!(h.eq_exact(one, two));
        assert_eq!(h.cmp_term(big, big), Ordering::Equal);
    }
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "the same term was read: {took:?}");
    let same_bytes = h.binary(&bytes);
    let other = h.binary(&vec![0xa5u8; 64 << 20]);
    assert!(h.eq_exact(big, same_bytes), "equal bytes are equal");
    assert!(!h.eq_exact(big, other), "different bytes differ");
    assert_eq!(h.cmp_term(big, other), Ordering::Less);
}
