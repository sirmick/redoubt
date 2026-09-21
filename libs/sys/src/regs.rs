//! Reading and writing a sequence of 64-bit slots in order: the eight argument registers (each
//! widened to a `u64`) or a record's slots.
//!
//! The register layout is the same on both widths: a `u64` takes two registers (low half, high
//! half), so every register holds at most 32 bits or one `usize`, and rv32 can carry it. In a
//! record a `u64` takes one slot.

use crate::Error;

/// Registers a call carries: `a0..=a7`.
pub const REGS: usize = 8;

/// Writes values into slots in order. Encoders never take untrusted input.
pub(crate) struct Writer<'a> {
    slots: &'a mut [u64],
    next: usize,
    /// Registers: a `u64` takes two slots. Records: one.
    split: bool,
}

impl<'a> Writer<'a> {
    /// Starts at `regs[0]`; `regs` should be all zero, so unused registers stay 0.
    pub fn regs(regs: &'a mut [u64; REGS]) -> Self { Writer { slots: regs, next: 0, split: true } }

    pub fn record(slots: &'a mut [u64]) -> Self { Writer { slots, next: 0, split: false } }

    /// Panics only if an encoding is longer than its array: a bug in this crate, which the
    /// round-trip tests would catch, never something input can cause.
    fn put(&mut self, value: u64) {
        self.slots[self.next] = value;
        self.next += 1;
    }

    pub fn u32(&mut self, value: u32) { self.put(value.into()) }

    pub fn u64(&mut self, value: u64) {
        if self.split {
            self.u32(value as u32);
            self.u32((value >> 32) as u32);
        } else {
            self.put(value);
        }
    }

    pub fn usize(&mut self, value: usize) { self.put(value as u64) }
}

/// Reads values from slots in order, rejecting any that do not fit their field.
pub(crate) struct Reader<'a> {
    slots: &'a [u64],
    next: usize,
    split: bool,
}

impl<'a> Reader<'a> {
    pub fn regs(regs: &'a [u64; REGS]) -> Self { Reader { slots: regs, next: 0, split: true } }

    pub fn record(slots: &'a [u64]) -> Self { Reader { slots, next: 0, split: false } }

    /// One slot as it is. Reading past the end yields 0 rather than panicking.
    pub fn raw(&mut self) -> u64 {
        let value = self.slots.get(self.next).copied().unwrap_or(0);
        self.next += 1;
        value
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        u32::try_from(self.raw()).map_err(|_| Error::InvalidArgument)
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        if self.split {
            let low = self.u32()?;
            Ok(u64::from(low) | u64::from(self.u32()?) << 32)
        } else {
            Ok(self.raw())
        }
    }

    /// On rv32 a value above `u32::MAX` (possible only in a record slot) is `InvalidArgument`.
    pub fn usize(&mut self) -> Result<usize, Error> {
        usize::try_from(self.raw()).map_err(|_| Error::InvalidArgument)
    }

    /// A tag numbered from 1: `n` names `values[n - 1]`; 0 and anything past the end are
    /// `InvalidArgument`.
    pub fn tag<T: Copy>(&mut self, values: &[T]) -> Result<T, Error> {
        let raw = self.raw();
        let index = raw.checked_sub(1).and_then(|i| usize::try_from(i).ok());
        index.and_then(|i| values.get(i)).copied().ok_or(Error::InvalidArgument)
    }

    /// Everything not read must be 0, so a malformed value cannot hide in an unused slot and
    /// every valid encoding is the only encoding of its value.
    pub fn finish(self) -> Result<(), Error> {
        let rest = self.slots.get(self.next..).unwrap_or(&[]);
        if rest.iter().all(|r| *r == 0) { Ok(()) } else { Err(Error::InvalidArgument) }
    }
}
