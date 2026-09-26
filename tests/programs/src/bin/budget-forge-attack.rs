//! Attacker: forge handle indices. It holds root, system and users in slots 1-3, a handle per
//! device object (WP-K3), and a budget of its own; it passes every other index it can think of
//! (0, every unused slot of the first
//! table page, indices on pages it does not have, which a kernel that dropped the page number
//! would read as slots 1-3, past the table, wider than 32 bits) to `budget_destroy` and the other
//! calls. A forged index that reached `system` would destroy it and kill the victim living there;
//! the victim reporting afterwards is the verdict. See `tests/budget-forge-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, Number};
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's third program: its budgets come from log-server, once, and no device (R2).
    let rd::Gifts { system, .. } = rd::take_gifts().expect("the budgets");
    log!(logger, "[attacker] starting");
    let own = rd::create(system, &rd::spec(10, 0, 0)).expect("own");
    rd::close(own).expect("close");
    // `own` is now a closed index, the last in use; those after it to 128 were never used;
    // 129..=133 and 257..=261 alias slots 1-5 of pages 1 and 2, which do not exist (on page 0 those
    // are the boot and log endpoints and the three gifts); 4096 is the last index, 4097 past the
    // table.
    let mut refused = 0;
    let mut tried = 0;
    let mut forged = [0u32; 140];
    let mut n = 0;
    for h in (own..=128).chain([129, 130, 131, 132, 133, 257, 258, 259, 260, 261, 4096, 4097, 0x8000_0001, u32::MAX]) {
        forged[n] = h;
        n += 1;
    }
    for &h in &forged[..n] {
        let results = [
            rd::destroy(h),
            rd::usage(h).map(|_| ()),
            rd::create(h, &rd::spec(1, 0, 0)).map(|_| ()),
            rd::close(h),
        ];
        tried += results.len();
        refused += results.iter().filter(|r| **r == Err(Error::BadHandle)).count();
        if results.iter().any(|r| *r != Err(Error::BadHandle)) {
            log!(logger, "[forge] index {} -> {:?}", h, results);
        }
    }
    // How many indices there are to forge depends on how many handles the machine gave this
    // program to start with, so what the case pins is that every one of them was refused.
    log!(logger, "[forge] {} of {} calls on forged indices got BadHandle ({})", refused, tried,
        if refused == tried { "all" } else { "FAIL" });
    // Indices that are not indices at all: 0, and (where registers are wide) above 32 bits.
    let destroy = rd::number(Number::BudgetDestroy);
    // Where registers are 64 bits wide, an index whose low 32 bits name `system`: a kernel that
    // truncated would destroy it. (On rv32 there is no such value.)
    let wide = match 1usize.checked_shl(32) {
        Some(bit) => rd::raw_error(rd::raw([destroy, bit | system as usize, 0, 0, 0, 0, 0, 0])),
        None => Some(Error::BadHandle),
    };
    let raw = [
        rd::raw_error(rd::raw([destroy, 0, 0, 0, 0, 0, 0, 0])),
        rd::raw_error(rd::raw([destroy, usize::MAX, 0, 0, 0, 0, 0, 0])),
        wide,
    ];
    log!(logger, "[forge] raw index 0, all ones, 2^32 + {} -> {:?}", system, raw);
    log!(logger, "[forge] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
