//! Reading and writing a sequence of registers (or buffer slots) in order.
//!
//! Generic over the register type so the host tests run both widths' encodings. Buffers use the
//! same code with `u64` slots, which is why a buffer's layout is the same on both widths.

use crate::Error;

/// Registers a call carries: `a0..=a7`.
pub const REGS: usize = 8;

/// A machine register: `u64` on rv64, `u32` on rv32.
pub trait Register: Copy + Eq + core::fmt::Debug {
    const ZERO: Self;
    /// Registers one `u64` takes: 1, or 2 (low half first) when registers are 32 bits.
    const PER_U64: usize;
    /// The low bits of `value` that fit in a register.
    fn truncate(value: u64) -> Self;
    fn widen(self) -> u64;
}

impl Register for u64 {
    const PER_U64: usize = 1;
    const ZERO: Self = 0;

    fn truncate(value: u64) -> Self { value }

    fn widen(self) -> u64 { self }
}

impl Register for u32 {
    const PER_U64: usize = 2;
    const ZERO: Self = 0;

    fn truncate(value: u64) -> Self { value as u32 }

    fn widen(self) -> u64 { self.into() }
}

/// Writes values into registers in order. Writing past the end is dropped; every encoding is
/// short enough (the tests check each one on 32-bit registers, where they are longest).
pub(crate) struct Writer<'a, R> {
    regs: &'a mut [R],
    next: usize,
}

impl<'a, R: Register> Writer<'a, R> {
    /// Starts at `regs[0]`; `regs` should be all zero, so unused registers stay 0.
    pub fn new(regs: &'a mut [R]) -> Self { Writer { regs, next: 0 } }

    /// Registers written so far (including any dropped past the end).
    #[cfg(test)]
    pub fn used(&self) -> usize { self.next }

    fn put(&mut self, value: u64) {
        if let Some(reg) = self.regs.get_mut(self.next) {
            *reg = R::truncate(value);
        }
        self.next += 1;
    }

    pub fn u64(&mut self, value: u64) {
        self.put(value);
        if R::PER_U64 == 2 {
            self.put(value >> 32);
        }
    }

    pub fn u32(&mut self, value: u32) { self.put(value.into()) }

    /// Exact on the target, where `usize` is as wide as a register. (The host tests' 32-bit
    /// encodings would truncate a value above `u32::MAX`; they use none.)
    pub fn usize(&mut self, value: usize) { self.put(value as u64) }
}

/// Reads values from registers in order, rejecting any that do not fit their field.
pub(crate) struct Reader<'a, R> {
    regs: &'a [R],
    next: usize,
}

impl<'a, R: Register> Reader<'a, R> {
    pub fn new(regs: &'a [R]) -> Self { Reader { regs, next: 0 } }

    /// One register as it is. Reading past the end yields 0 rather than panicking; no decoding does it.
    pub fn raw(&mut self) -> u64 {
        let value = self.regs.get(self.next).map_or(0, |r| r.widen());
        self.next += 1;
        value
    }

    pub fn u64(&mut self) -> u64 {
        let low = self.raw();
        if R::PER_U64 == 2 { low | self.raw() << 32 } else { low }
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        u32::try_from(self.raw()).map_err(|_| Error::InvalidArgument)
    }

    pub fn usize(&mut self) -> Result<usize, Error> {
        usize::try_from(self.raw()).map_err(|_| Error::InvalidArgument)
    }

    /// Everything not read must be 0, so a malformed value cannot hide in an unused register
    /// and every valid encoding is the only encoding of its value.
    pub fn finish(self) -> Result<(), Error> {
        let rest = self.regs.get(self.next..).unwrap_or(&[]);
        if rest.iter().all(|r| *r == R::ZERO) { Ok(()) } else { Err(Error::InvalidArgument) }
    }
}
