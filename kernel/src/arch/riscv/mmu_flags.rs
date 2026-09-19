// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Page table entry flags. Sv32 and Sv39 entries use the same low ten bits, including
//! the two software bits Xous uses for lent (`S`) and swapped (`P`) pages, so this is
//! shared by `mem.rs` and `mem_sv39.rs`.

use xous_kernel::MemoryFlags;

// On rv64 the flags are the `sv39` crate's, so that the loader and the kernel agree.
#[cfg(target_arch = "riscv64")]
pub use paging::PteFlags as MMUFlags;

#[cfg(not(target_arch = "riscv64"))]
bitflags! {
    pub struct MMUFlags: usize {
        const NONE      = 0b00_0000_0000;
        const VALID     = 0b00_0000_0001;
        const R         = 0b00_0000_0010;
        const W         = 0b00_0000_0100;
        const X         = 0b00_0000_1000;
        const USER      = 0b00_0001_0000;
        const GLOBAL    = 0b00_0010_0000;
        const A         = 0b00_0100_0000;
        const D         = 0b00_1000_0000;
        const S         = 0b01_0000_0000; // Shared page
        const P         = 0b10_0000_0000; // swaP
    }
}

pub fn translate_flags(req_flags: MemoryFlags) -> MMUFlags {
    let mut flags = MMUFlags::NONE;

    // TODO for vex-ii:
    // Vexii implement A-flag. In this case, we should not just be setting every
    // readable page to "A", we should add a handler in the IRQ handler that sets "A"
    // when the page is actually read.
    if req_flags & xous_kernel::MemoryFlags::R == xous_kernel::MemoryFlags::R {
        flags |= MMUFlags::R;
    }

    // TODO for vex-ii:
    // Vexii implement D-flag. In this case, we should not just be setting every
    // writeable page to "D", we should add a handler in the IRQ handler that sets "D"
    // when the page is actually writte.
    if req_flags & xous_kernel::MemoryFlags::W == xous_kernel::MemoryFlags::W {
        flags |= MMUFlags::W;
    }

    if req_flags & xous_kernel::MemoryFlags::X == xous_kernel::MemoryFlags::X {
        flags |= MMUFlags::X;
    }
    if req_flags & xous_kernel::MemoryFlags::P == xous_kernel::MemoryFlags::P {
        flags |= MMUFlags::P;
    }
    flags
}

pub fn untranslate_flags(req_flags: usize) -> MemoryFlags {
    let req_flags = MMUFlags::from_bits_truncate(req_flags);
    let mut flags = xous_kernel::MemoryFlags::FREE;
    if req_flags & MMUFlags::R == MMUFlags::R {
        flags |= xous_kernel::MemoryFlags::R;
    }
    if req_flags & MMUFlags::W == MMUFlags::W {
        flags |= xous_kernel::MemoryFlags::W;
    }
    if req_flags & MMUFlags::X == MMUFlags::X {
        flags |= xous_kernel::MemoryFlags::X;
    }
    if req_flags & MMUFlags::P == MMUFlags::P {
        flags |= xous_kernel::MemoryFlags::P;
    }
    flags
}
