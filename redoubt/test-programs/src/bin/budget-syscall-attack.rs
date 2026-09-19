//! Attacker: hostile arguments to every call WP-K1 built (I14): bad and wide handles, unknown
//! numbers, non-zero unused registers, records misaligned, at 0, in the kernel, read-only,
//! never touched, straddling into an unmapped page, malformed slot by slot, the largest values; the order in
//! which the stages report them; then a few thousand calls with arguments drawn from a pool of
//! hostile values. No argument may panic the kernel. Each targeted attempt's error is printed as
//! progress; the verdict is the victim's report and the checker's power-off, which a panicked
//! kernel never reaches. See `redoubt/tests/budget-syscall-attack.toml`.

#![no_std]
#![no_main]

use redoubt_sys::{BUDGET_SPEC_SLOTS, NUMBER_BASE};
use test_programs::rd::{self, Error, Number};
use test_programs::{Logger, log};
use xous::MemoryFlags;

/// Where the fuzzing calls may write: nothing else of this program's.
static mut SCRATCH: [u64; 1024] = [0; 1024];

/// An address in the kernel's half on both widths.
const KERNEL: usize = usize::MAX & !0xfff;

fn call(n: Number, args: [usize; 7]) -> Option<Error> {
    let a = args;
    rd::raw_error(rd::raw([rd::number(n), a[0], a[1], a[2], a[3], a[4], a[5], a[6]]))
}

fn good_spec() -> [u64; BUDGET_SPEC_SLOTS] { rd::spec(1, 0, 0).encode() }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[attacker] starting");
    let scratch = (&raw mut SCRATCH) as usize;
    // Decoding never backs an untouched page (QUESTIONS.md 115), so touch every page of SCRATCH.
    for word in (0..1024).step_by(512) {
        // SAFETY: an in-bounds word of SCRATCH, this program's own.
        unsafe { ((&raw mut SCRATCH) as *mut u64).add(word).write_volatile(0) };
    }
    let text = _start as *const () as usize & !7;
    // A page mapped, then one unmapped right after it.
    let pages = xous::map_memory(None, None, 8192, MemoryFlags::R | MemoryFlags::W).expect("map");
    let end = pages.as_ptr() as usize + 4096;
    // SAFETY: unmapping the second page of this program's own fresh mapping.
    xous::unmap_memory(unsafe { xous::MemoryRange::new(end, 4096) }.unwrap()).expect("unmap");
    // SAFETY: the first page is still mapped, writable and this program's.
    unsafe { (pages.as_mut_ptr() as *mut u64).write_volatile(1) };

    let create = |rec: usize| call(Number::BudgetCreate, [rd::SYSTEM as usize, rec, 0, 0, 0, 0, 0]);
    let spec = good_spec();
    let at = |slots: &[u64; BUDGET_SPEC_SLOTS]| {
        for (i, slot) in slots.iter().enumerate() {
            // SAFETY: an in-bounds word of SCRATCH, which is this program's; nothing else uses it
            // while a call runs.
            unsafe { ((&raw mut SCRATCH) as *mut u64).add(i).write_volatile(*slot) };
        }
        scratch
    };
    let mut bad_class = spec;
    bad_class[3] = 3;
    let mut zero_class = spec;
    zero_class[3] = 0;
    let mut nine_labels = spec;
    nine_labels[4] = 9;
    let mut stray_label = spec;
    stray_label[5] = 7; // count 0, slot non-zero
    let mut wide_processes = spec;
    wide_processes[1] = 1 << 32;
    let mut huge_pages = spec;
    huge_pages[0] = u64::MAX;
    let mut huge_weight = spec;
    huge_weight[2] = u32::MAX as u64;

    let records = [
        create(scratch + 4),
        create(0),
        create(KERNEL),
        create(end - 8),
        create(at(&bad_class)),
        create(at(&zero_class)),
        create(at(&nine_labels)),
        create(at(&stray_label)),
        create(at(&wide_processes)),
        create(at(&huge_pages)),
        create(at(&huge_weight)),
    ];
    log!(logger, "[i14] budget_create records -> {:?}", records);
    let order = [
        call(Number::BudgetCreate, [0, scratch + 4, 0, 0, 0, 0, 0]),
        call(Number::BudgetCreate, [999, scratch + 4, 0, 0, 0, 0, 0]),
        call(Number::BudgetCreate, [999, at(&spec), 0, 0, 0, 0, 0]),
        call(Number::BudgetCreate, [rd::SYSTEM as usize, at(&spec), 1, 0, 0, 0, 0]),
    ];
    log!(logger, "[i14] budget_create order -> {:?}", order);
    // A page reserved and never touched: decoding does not back it (QUESTIONS.md 115).
    let untouched = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W).expect("map");
    let usage = [
        call(Number::BudgetUsage, [rd::SYSTEM as usize, untouched.as_ptr() as usize, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [rd::SYSTEM as usize, text, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [rd::SYSTEM as usize, scratch + 1, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [rd::SYSTEM as usize, KERNEL, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [rd::SYSTEM as usize, end - 8, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [999, text, 0, 0, 0, 0, 0]),
        call(Number::BudgetUsage, [999, scratch, 0, 0, 0, 0, 0]),
    ];
    log!(logger, "[i14] budget_usage -> {:?}", usage);
    let random = [
        call(Number::Random, [text, 8, 0, 0, 0, 0, 0]),
        call(Number::Random, [KERNEL, 8, 0, 0, 0, 0, 0]),
        call(Number::Random, [usize::MAX - 3, 8, 0, 0, 0, 0, 0]),
        call(Number::Random, [0, 1, 0, 0, 0, 0, 0]),
        call(Number::Random, [end - 4, 8, 0, 0, 0, 0, 0]),
        call(Number::Random, [scratch, 65, 0, 0, 0, 0, 0]),
        call(Number::Random, [scratch, usize::MAX, 0, 0, 0, 0, 0]),
    ];
    log!(logger, "[i14] random -> {:?}", random);
    let base = NUMBER_BASE as usize;
    let other = [
        rd::raw_error(rd::raw([base, 0, 0, 0, 0, 0, 0, 0])),
        rd::raw_error(rd::raw([base + 25, 0, 0, 0, 0, 0, 0, 0])),
        rd::raw_error(rd::raw([usize::MAX, 0, 0, 0, 0, 0, 0, 0])),
        call(Number::TimeNow, [1, 0, 0, 0, 0, 0, 0]),
        call(Number::TimeNow, [0, 0, 0, 0, 0, 0, 1]),
        call(Number::HandleClose, [0, 0, 0, 0, 0, 0, 0]),
        call(Number::HandleClose, [999, 0, 0, 0, 0, 0, 0]),
        call(Number::HandleClose, [rd::SYSTEM as usize, 1, 0, 0, 0, 0, 0]),
        call(Number::BudgetDestroy, [999, 0, 0, 0, 0, 0, 0]),
        // Calls not built yet (WP-K2 to WP-K5) decode, then are refused.
        call(Number::MapAnon, [4096, 3, 0, 0, 0, 0, 0]),
        call(Number::EndpointCreate, [0, 0, 0, 0, 0, 0, 0]),
    ];
    log!(logger, "[i14] other -> {:?}", other);
    let sandbox = rd::create(rd::SYSTEM, &rd::spec(200, 0, 0)).expect("sandbox");

    // Random calls over hostile values. Never destroy or close slots 1-3: losing `system` would
    // kill this program and the victim, which is not the attack.
    let pool = [
        0, 1, 2, 3, 4, 7, 8, 64, 65, 0xfff, 0x1000, scratch, scratch + 3, scratch + 4096, text, end - 8, end,
        KERNEL, u32::MAX as usize, usize::MAX, usize::MAX - 7, 1 << 31,
    ];
    let mut seed = 0x9e37_79b9_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let (mut calls, mut refused, mut odd) = (0, 0, 0);
    for _ in 0..4000 {
        let number = Number::ALL[next() as usize % Number::ALL.len()];
        let mut args = [0usize; 7];
        for arg in args.iter_mut() {
            *arg = if next() % 4 == 0 { next() as usize } else { pool[next() as usize % pool.len()] };
        }
        if matches!(number, Number::BudgetDestroy | Number::HandleClose) && (1..=3).contains(&args[0]) {
            continue;
        }
        // New budgets come only from the sandbox, so the fuzzing cannot use up `system`.
        if number == Number::BudgetCreate && (1..=3).contains(&args[0]) {
            args[0] = sandbox as usize;
        }
        // Where a call writes, the address is from the pool: every writable address in it is
        // SCRATCH or past the mapped page, never this program's stack or data.
        match number {
            Number::Random => args[0] = pool[next() as usize % pool.len()],
            Number::BudgetUsage => args[1] = pool[next() as usize % pool.len()],
            _ => {}
        }
        calls += 1;
        let a0 = rd::raw([rd::number(number), args[0], args[1], args[2], args[3], args[4], args[5], args[6]]);
        match rd::raw_error(a0) {
            Some(_) => refused += 1,
            None if a0 == 0 => {}
            None => odd += 1,
        }
    }
    log!(logger, "[i14] {} random calls: {} refused, {} with an unknown result code", calls, refused, odd);
    log!(logger, "[i14] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
