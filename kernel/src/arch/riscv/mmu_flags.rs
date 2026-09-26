// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Page table entry flags. Sv32 and Sv39 entries use the same low ten bits, including the
//! software bit Redoubt uses for lent (`S`) pages, so the flags are the `paging` crate's
//! `PteFlags` on both widths, and the loader and kernel agree.

use paging::PteFlags;
use redoubt_sys::MemFlags;

/// The entry permissions for a mapping's `flags`.
pub fn translate_flags(flags: MemFlags) -> PteFlags {
    let mut pte = PteFlags::NONE;
    let bits = [
        (MemFlags::READ, PteFlags::R),
        (MemFlags::WRITE, PteFlags::W),
        (MemFlags::EXECUTE, PteFlags::X),
    ];
    for (flag, bit) in bits {
        if flags.contains(flag) {
            pte |= bit;
        }
    }
    pte
}
