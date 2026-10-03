//! The userspace side of a call on the machine: the `ecall` itself. The only `unsafe` in this
//! crate.
#![allow(unsafe_code)]

use crate::{Call, Error, Return, Transport, decode_result};

/// The machine's transport: a call is an `ecall`, its arguments and result in `a0..=a7` (the
/// crate docs, Registers). The kernel checks that the records and buffers named in a call are
/// mapped.
pub struct Ecall;

// SAFETY: the kernel is the implementation: it checks every record and buffer a call names and
// returns only memory mapped in the caller.
unsafe impl Transport for Ecall {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        // Every register holds at most 32 bits or one `usize` (regs.rs; the tests' `fits_rv32`
        // checks it for every call), so the casts are exact on both widths.
        let mut regs = call.encode().map(|r| r as usize);
        // SAFETY: `ecall` traps to the kernel, which reads a0-a7, performs the call and writes its
        // result to a0-a7, preserving every other register of this thread. Without `nomem`, the
        // compiler assumes memory may change, which is right: the kernel writes the records and
        // buffers named in `call` (whose addresses were exposed by the `as usize` casts that made
        // them).
        unsafe {
            core::arch::asm!(
                "ecall",
                inlateout("a0") regs[0],
                inlateout("a1") regs[1],
                inlateout("a2") regs[2],
                inlateout("a3") regs[3],
                inlateout("a4") regs[4],
                inlateout("a5") regs[5],
                inlateout("a6") regs[6],
                inlateout("a7") regs[7],
                options(nostack),
            );
        }
        decode_result(call.number(), &regs.map(|r| r as u64))
    }
}

/// Makes `call` through the [`Ecall`], for what sits below the runtime by design: the stub, its
/// fixture and the test programs (redoubt-rt has its own seam). For IPC `call`, errors are in
/// `Return::Call(...).status`, alongside ownership and reply validity; outer `Err` means a
/// malformed outcome. Never discard the call outcome.
pub fn syscall(call: &Call) -> Result<Return, Error> { Ecall.call(call) }
