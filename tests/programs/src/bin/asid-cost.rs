//! What a switch of address space and a page-table change cost (`tests/asid-cost.toml` in guest
//! instructions, `tests/asid-cost-host.toml` in host time; kernel/memory-layout.md, "`satp`"). A
//! record, not a bound: the case judges nothing but that the numbers are printed.
//!
//! An empty call and its reply, to a server thread in this process and to a server in a child
//! process: the second crosses into another address space and back, the first does not, so their
//! difference over the two switches each round trip makes is what a switch costs. Then a page
//! mapped, touched and unmapped beside one that stays. Each `COUNT` times.

#![no_std]
#![no_main]

use test_programs::rd::{self, Received};
use test_programs::{log, logsrv, spawn};

const COUNT: u64 = 10_000;
const WAIT: u64 = 2_000_000;
/// Where the map loop maps its page: between the message area and the `map_anon` area, clear of
/// the bundle the loader maps into this first program (memory-layout.md, "Regions").
const MAPPED: usize = 0x5800_0000;
/// The exit code of a panicking process (the handler below).
const PANICKED: u32 = 101;

/// Answer every call on `endpoint` with an empty reply, for ever.
fn serve(endpoint: usize) {
    loop {
        if let Ok(Received::Message(m)) = rd::receive(Some(endpoint as u32), rd::FOREVER, 0) {
            rd::reply(m.msg_id.get(), &rd::body([0; 4])).ok();
        }
    }
}

/// The child server: its endpoint is in slot 1.
extern "C" fn child_server(_: usize) -> ! {
    serve(1);
    rd::process_exit(0)
}

/// Microseconds for `count` empty calls on `endpoint`.
fn round_trips(endpoint: u32, count: u64) -> u64 {
    let t0 = rd::time_now().unwrap();
    for _ in 0..count {
        rd::call(endpoint, &rd::body([0; 4]), None, WAIT).expect("an empty call");
    }
    rd::time_now().unwrap() - t0
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();

    let here = rd::endpoint_create().expect("this process's endpoint");
    rd::thread(serve, here as usize).expect("the server thread");
    let here_send = rd::mint_from_handle(here, 1, None).expect("a send on it");

    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let kids = rd::create(rd::USERS, &rd::spec(600, 2, 100)).expect("the child's budget");
    let there = rd::endpoint_create().expect("the child's endpoint");
    let there_send = rd::mint_from_handle(there, 1, None).expect("a send on it");
    spawn::spawn(&image, kids, exit, child_server as *const () as usize, &[], &[there]).expect("the child");

    // Warm both paths before timing them.
    round_trips(here_send, COUNT / 10);
    round_trips(there_send, COUNT / 10);
    let within = round_trips(here_send, COUNT);
    let between = round_trips(there_send, COUNT);
    log!(logger, "[asid-cost] {} ipc round trips within a process: {} us", COUNT, within);
    log!(logger, "[asid-cost] {} ipc round trips between processes: {} us", COUNT, between);
    log!(
        logger,
        "[asid-cost] an address-space switch, from the difference: {} ns",
        between.saturating_sub(within) * 1000 / (2 * COUNT)
    );

    // A neighbour keeps the leaf table, so the loop changes a leaf and no table.
    rd::map_fixed(MAPPED + rd::PAGE_SIZE, rd::PAGE_SIZE, rd::rw()).expect("map the neighbour");
    let t0 = rd::time_now().unwrap();
    for _ in 0..COUNT {
        rd::map_fixed(MAPPED, rd::PAGE_SIZE, rd::rw()).expect("map the page");
        rd::poke(MAPPED, 1);
        rd::unmap(MAPPED, rd::PAGE_SIZE).expect("unmap the page");
    }
    let mapped = rd::time_now().unwrap() - t0;
    log!(logger, "[asid-cost] {} maps, touches and unmaps of a page: {} us", COUNT, mapped);
    log!(logger, "ASID COST RECORDED");
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { rd::process_exit(PANICKED) }
