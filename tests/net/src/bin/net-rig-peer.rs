//! The rig for bench-net-peer (`src/rig.rs`,
//! `Mode::Peer`). The bundle's first program: the kernel starts it directly, with no startup block.

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    redoubt_rt::start(|_| redoubt_net_tests::rig::run(redoubt_net_tests::rig::Mode::Peer), 0)
}

#[cfg(not(target_os = "none"))]
fn main() {}
