//! A test fixture, not part of the stub itself: the smallest possible ELF for `stub-launch`
//! (`tests/programs`, WP-R2) to run *through* the real stub. Links at the ordinary default
//! address (`build.rs` scopes `-Tstub.x` to the `stub` bin only), so this is exactly the shape
//! of program the stub is meant to load. On reaching its own entry it exits with [`OK`], which
//! only happens if the stub actually mapped its segments and jumped here.
#![no_std]
#![no_main]

use redoubt_rt::handle::process_exit;

/// Proves this program's own entry ran, not the stub's own exit codes (110/111) or the kernel's
/// default fault code (15).
pub const OK: u32 = 77;

#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! { process_exit(OK) }
