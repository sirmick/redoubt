//! The rig for d3-net-attacks (`src/rig.rs`, `Mode::Attacks`). The bundle's first program: the
//! kernel starts it directly, with no startup block.

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    redoubt_rt::start(|_| redoubt_net_tests::rig::run(redoubt_net_tests::rig::Mode::Attacks), 0)
}

#[cfg(not(target_os = "none"))]
fn main() {}
