//! `consoled` against WP-W1's 9P2000 conformance vectors (`redoubt/wire/vectors/9p.txt`). What
//! they check is in the runner's own docs; what is checked here on top is what only `consoled`
//! knows: a run of hostile and well-formed messages neither prints anything on the line nor
//! swallows a byte that was typed, and — with the line quiet — no vector leaves a call held,
//! because every one of them reaches a fid that was never opened for reading.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod common;
#[path = "../../../libs/rt/tests/common/vectors.rs"]
mod vectors;

use common::fake;
use redoubt_consoled::server::{Console, LIMITS};
use redoubt_consoled::uart::Uart;
use redoubt_rt::abi::Labels;
use redoubt_rt::handle::Mmio;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FIRST_MINTED_BADGE, NineServer};

const RBR_THR: usize = 0;
const LSR: usize = 5;
const LSR_THR_EMPTY: u8 = 0x20;
const REGISTERS: usize = 8;

#[test]
fn the_conformance_vectors_run_against_consoled() {
    let f = fake();
    let pid = f.process(0, &[]);
    let (mmio, _irq) = f.device(pid, REGISTERS);
    let regs = f.registers(pid, mmio);
    // The line is quiet and the transmitter has room: nothing to read, everything printable.
    regs[LSR] = LSR_THR_EMPTY;
    regs[RBR_THR] = 0;

    let mut server = f.as_process(pid, || {
        let uart = Uart::new(Mmio::from_handle(mmio).registers().unwrap()).unwrap();
        uart.init();
        NineServer::new(Console::new(uart), LIMITS, 0x0fed_cba9_8765_4321).unwrap()
    });
    // `init()` leaves the divisor latch's low byte in the transmit register; start from a clean
    // one so anything found there afterwards was printed by a vector.
    f.registers(pid, mmio)[RBR_THR] = 0;

    let who =
        Caller { badge: FIRST_MINTED_BADGE + 3, account: 1001, labels: Labels::from_slice(&[]).unwrap() };
    let counts = f.as_process(pid, || vectors::run(&mut server, &who));
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    // No vector opens a fid for reading, so none reaches the waiting path; the ones that would
    // read name fids that do not exist.
    assert_eq!(counts.waiting, 0, "a vector was held: {counts:?}");
    assert_eq!(f.registers(pid, mmio)[RBR_THR], 0, "a vector printed on the console");
    assert!(!server.fs.has_input(), "a vector invented input");
    assert_eq!(server.fs.dropped(), 0);
}
