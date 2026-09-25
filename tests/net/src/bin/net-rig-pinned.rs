//! The rig for d3-net-pinned (`src/rig.rs`,
//! `Mode::Pinned`). The bundle's first program: the kernel starts it directly, with no startup block.

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    redoubt_rt::start(|_| redoubt_net_tests::rig::run(redoubt_net_tests::rig::Mode::Pinned), 0)
}

#[cfg(not(target_os = "none"))]
fn main() {}
