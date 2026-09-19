// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Page table entry flags. Sv32 and Sv39 entries use the same low ten bits, including the
//! two software bits Xous uses for lent (`S`) and swapped (`P`) pages, so the flags are the
//! `paging` crate's `PteFlags` on both widths, and the loader and kernel agree.

use xous_kernel::MemoryFlags;

pub use paging::PteFlags as MMUFlags;

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
