//! A fixed set of small numbers, walked by set bit: a process's TIDs, the live PIDs and the PIDs
//! with a process object. A walk of one costs a word read per 64 numbers and a step per number in
//! the set, so a walk over the threads, processes or objects that exist does not pay for the limit.

/// A set of numbers below `64 * W`: bit `n & 63` of word `n >> 6` for `n`. The empty set is all
/// zeros, so a table of them stays `.bss`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bits<const W: usize>([u64; W]);

impl<const W: usize> Bits<W> {
    pub const EMPTY: Self = Bits([0; W]);

    /// The set of `n` alone.
    pub fn of(n: usize) -> Self { Self::EMPTY.with(n) }

    pub fn contains(&self, n: usize) -> bool { self.0[n >> 6] & 1 << (n & 63) != 0 }

    pub fn with(mut self, n: usize) -> Self {
        self.0[n >> 6] |= 1 << (n & 63);
        self
    }

    pub fn without(mut self, n: usize) -> Self {
        self.0[n >> 6] &= !(1 << (n & 63));
        self
    }

    pub fn is_empty(&self) -> bool { self.0.iter().all(|word| *word == 0) }

    /// The numbers in the set, lowest first.
    pub fn iter(self) -> impl Iterator<Item = usize> {
        (0..W).flat_map(move |i| {
            let mut word = self.0[i];
            core::iter::from_fn(move || {
                let bit = (word != 0).then(|| word.trailing_zeros() as usize)?;
                word &= word - 1;
                Some(i * 64 + bit)
            })
        })
    }
}

impl<const W: usize> core::fmt::Debug for Bits<W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.iter().rev().try_for_each(|word| write!(f, "{:016x}", word))
    }
}
