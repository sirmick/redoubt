//! A test fixture, not part of the stub itself: the smallest possible ELF for `stub-launch`
//! (`tests/programs`) to run *through* the real stub. Links at the ordinary default
//! address (`build.rs` scopes `-Tstub.x` to the `stub` bin only), so this is exactly the shape
//! of program the stub is meant to load. On reaching its own entry it checks that the stub freed
//! the image copy, then exits with [`OK`], which only happens if the stub actually mapped its
//! segments, unmapped the copy and jumped here.
#![no_std]
#![no_main]

use core::panic::PanicInfo;

use redoubt_sys::{Call, MemFlags, PAGE_SIZE, syscall};
use stub::{IMAGE_AT, process_exit};

/// Proves this program's own entry ran, not the stub's own exit codes (110 to 112) or the
/// kernel's default fault code (15).
pub const OK: u32 = 77;
/// The image copy was still mapped when this program started: the stub did not free it.
pub const IMAGE_STILL_MAPPED: u32 = 78;

#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    // `stub-launch` copied the image to `IMAGE_AT`. `map_fixed` never replaces a mapping (kernel/memory.md),
    // so it succeeds on the copy's first page only if the stub unmapped it before the jump
    // (servers/init.md, launch step 5).
    let free = syscall(&Call::MapFixed { addr: IMAGE_AT, len: PAGE_SIZE, flags: MemFlags::READ });
    process_exit(if free.is_ok() { OK } else { IMAGE_STILL_MAPPED })
}

/// This binary's one panic handler: like the stub's own, never reached in the fixture's own
/// straight-line `_start` above, but required for any `no_std`/`no_main` binary in this crate
/// (stub/src/main.rs never links `redoubt-rt`, so neither does this fixture).
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! { process_exit(101) }

/// See `stub::NullAlloc`'s doc: linking the `stub` lib crate (via `redoubt-wire`) needs a
/// `#[global_allocator]` even though this fixture never allocates.
#[global_allocator]
static ALLOC: stub::NullAlloc = stub::NullAlloc;
