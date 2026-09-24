//! The rig for bench-net-peer-twice, bench-net-peer-count and bench-net-peer-pcap-empty (`src/rig.rs`,
//! `Mode::Twice`). The bundle's first program: the kernel starts it directly, with no startup block.

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    redoubt_rt::start(|_| redoubt_net_tests::rig::run(redoubt_net_tests::rig::Mode::Twice), 0)
}

#[cfg(not(target_os = "none"))]
fn main() {}
