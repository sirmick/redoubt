// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Page table entry flags. Sv32 and Sv39 entries use the same low ten bits, including the
//! two software bits Redoubt uses for lent (`S`) and swapped (`P`) pages, so the flags are the
//! `paging` crate's `PteFlags` on both widths, and the loader and kernel agree.

use redoubt_abi::MemoryFlags;

pub use paging::PteFlags as MMUFlags;

pub fn translate_flags(req_flags: MemoryFlags) -> MMUFlags {
    let mut flags = MMUFlags::NONE;

    // TODO for vex-ii:
    // Vexii implement A-flag. In this case, we should not just be setting every
    // readable page to "A", we should add a handler in the IRQ handler that sets "A"
    // when the page is actually read.
    if req_flags & redoubt_abi::MemoryFlags::R == redoubt_abi::MemoryFlags::R {
        flags |= MMUFlags::R;
    }

    // TODO for vex-ii:
    // Vexii implement D-flag. In this case, we should not just be setting every
    // writeable page to "D", we should add a handler in the IRQ handler that sets "D"
    // when the page is actually writte.
    if req_flags & redoubt_abi::MemoryFlags::W == redoubt_abi::MemoryFlags::W {
        flags |= MMUFlags::W;
    }

    if req_flags & redoubt_abi::MemoryFlags::X == redoubt_abi::MemoryFlags::X {
        flags |= MMUFlags::X;
    }
    if req_flags & redoubt_abi::MemoryFlags::P == redoubt_abi::MemoryFlags::P {
        flags |= MMUFlags::P;
    }
    flags
}

pub fn untranslate_flags(req_flags: usize) -> MemoryFlags {
    let req_flags = MMUFlags::from_bits_truncate(req_flags);
    let mut flags = redoubt_abi::MemoryFlags::FREE;
    if req_flags & MMUFlags::R == MMUFlags::R {
        flags |= redoubt_abi::MemoryFlags::R;
    }
    if req_flags & MMUFlags::W == MMUFlags::W {
        flags |= redoubt_abi::MemoryFlags::W;
    }
    if req_flags & MMUFlags::X == MMUFlags::X {
        flags |= redoubt_abi::MemoryFlags::X;
    }
    if req_flags & MMUFlags::P == MMUFlags::P {
        flags |= redoubt_abi::MemoryFlags::P;
    }
    flags
}
