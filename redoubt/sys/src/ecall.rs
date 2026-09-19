//! The userspace side of a call: the `ecall` itself. The only `unsafe` in this crate.
#![allow(unsafe_code)]

use crate::{Call, Error, REGS, Reg, Return, decode_result};

/// Makes `call` and decodes its result. Buffers named by address in `call` must stay valid (and,
/// for results, writable) for the duration of the call; the kernel checks that they are mapped.
pub fn syscall(call: &Call) -> Result<Return, Error> {
    let mut regs: [Reg; REGS] = call.encode();
    // SAFETY: `ecall` traps to the kernel, which reads a0-a7, performs the call and writes its
    // result to a0-a7, changing no other register of this thread. Without `nomem`, the compiler
    // assumes memory may change, which is right: the kernel writes the buffers named in `call`
    // (whose addresses were exposed by the `as usize` casts that made them).
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
    decode_result(call.number(), &regs)
}
