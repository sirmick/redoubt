//! Trusted IPC1 checker. Its serving thread changes a blocked caller's record at a known
//! boundary, then replies; the main checker judges kernel outcomes and accounting, not text
//! from a hostile program. Existing redoubt-ipc covers separate address spaces.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_sys::{CallOutcome, LendDisposition, ReplyOutcome};
use test_programs::rd::{self, Error, FOREVER, Received, Return};
use test_programs::{Logger, log};

static ENDPOINT: AtomicUsize = AtomicUsize::new(0);
static TAKEN: AtomicUsize = AtomicUsize::new(0);
static REPLIES: AtomicUsize = AtomicUsize::new(0);
static DELIVERY: AtomicUsize = AtomicUsize::new(0);
static MASK: AtomicUsize = AtomicUsize::new(0);
const ECHO: usize = 1;
const READ_ONLY: usize = 2;
const UNMAP: usize = 3;
const HANDLES: usize = 4;
const REVOKE: usize = 5;
const DIE: usize = 6;
const REMAP: usize = 7;
const LOAN_PROTECTION: usize = 8;

fn protected_alias(addr: usize, endpoint: u32) {
    assert_eq!(rd::unmap(addr, rd::PAGE_SIZE), Err(Error::InvalidArgument), "loan alias unmap");
    assert_eq!(rd::set_flags(addr, rd::PAGE_SIZE, rd::MemFlags::READ), Err(Error::InvalidArgument));
    // SAFETY: `addr` is the page-aligned one-page loan mapping supplied by the kernel.
    let range = unsafe { redoubt_abi::MemoryRange::new(addr, rd::PAGE_SIZE) }.unwrap();
    assert_eq!(redoubt_abi::unmap_memory(range), Err(redoubt_abi::Error::ShareViolation));
    let (out, _) = rd::call_outcome(endpoint, &rd::body([0; 4]), rd::pages(addr, 1), 0).unwrap();
    assert_eq!(out.status, Err(Error::InvalidArgument), "loan alias re-lend");
    assert_eq!(rd::send(endpoint, &rd::body([0; 4]), rd::pages(addr, 1), 0), Err(Error::InvalidArgument));
}

fn server(_: usize) {
    let endpoint = ENDPOINT.load(Ordering::Acquire) as u32;
    loop {
        let Received::Message(m) = rd::receive(Some(endpoint), FOREVER, 0).expect("receive") else {
            continue;
        };
        TAKEN.fetch_add(1, Ordering::Release);
        let op = m.body.words[0];
        match op {
            READ_ONLY => rd::set_flags(m.body.words[1], rd::PAGE_SIZE, rd::MemFlags::READ).unwrap(),
            UNMAP => rd::unmap(m.body.words[1], rd::PAGE_SIZE).unwrap(),
            REMAP => {
                let addr = m.body.words[1];
                rd::unmap(addr, rd::PAGE_SIZE).unwrap();
                redoubt_abi::map_memory(
                    None,
                    redoubt_abi::MemoryAddress::new(addr),
                    rd::PAGE_SIZE,
                    redoubt_abi::MemoryFlags::R | redoubt_abi::MemoryFlags::W,
                )
                .unwrap();
                rd::poke(addr, 0xface);
                rd::set_flags(addr, rd::PAGE_SIZE, rd::MemFlags::READ).unwrap();
            }
            LOAN_PROTECTION | REVOKE => {
                let redoubt_sys::MessageKind::Call { lend: Some(lend) } = m.kind else {
                    panic!("expected loan")
                };
                let before = rd::usage(rd::SYSTEM).unwrap().pages_usage;
                protected_alias(lend.addr, endpoint);
                if op == LOAN_PROTECTION {
                    protected_alias(m.body.words[1], endpoint);
                    assert_eq!(rd::peek(lend.addr), 42);
                    rd::poke(lend.addr, 0xbeef);
                } else {
                    rd::destroy(m.body.words[1] as u32).unwrap();
                    // Once abandoned the receiver owns the charge, not the right to free
                    // or transfer the still-open loan before the kernel's reply cleanup.
                    protected_alias(lend.addr, endpoint);
                }
                if op == LOAN_PROTECTION {
                    assert_eq!(rd::usage(rd::SYSTEM).unwrap().pages_usage, before);
                }
            }
            DIE => return,
            _ => {}
        }
        let body = if matches!(op, READ_ONLY | UNMAP | REMAP | HANDLES | REVOKE) {
            rd::body_with([42, 43, 0, 0], &[endpoint, endpoint])
        } else {
            rd::body([42, 43, 0, 0])
        };
        let rec = body.encode();
        let Return::Reply(outcome) =
            redoubt_sys::syscall(&rd::Call::Reply { msg_id: m.msg_id, body_rec: rec.as_ptr() as usize })
                .expect("reply")
        else {
            panic!("reply ABI")
        };
        DELIVERY.store(usize::from(outcome.delivered), Ordering::Release);
        MASK.store(outcome.installed as usize, Ordering::Release);
        REPLIES.fetch_add(1, Ordering::Release);
    }
}

fn replied(count: usize) -> ReplyOutcome {
    while REPLIES.load(Ordering::Acquire) < count {
        test_programs::wait_ms(1);
    }
    ReplyOutcome {
        delivered: DELIVERY.load(Ordering::Acquire) != 0,
        installed: MASK.load(Ordering::Acquire) as u32,
    }
}

fn write_body(at: usize, body: rd::Body) {
    // SAFETY: the checker owns this mapped writable page and uses no alias during the write.
    unsafe {
        (at as *mut [u64; redoubt_sys::BODY_SLOTS]).write(body.encode());
    }
}

fn raw_call(endpoint: u32, record: usize, lend: Option<rd::Pages>) -> CallOutcome {
    let Return::Call(outcome) = redoubt_sys::syscall(&rd::Call::Call {
        endpoint: rd::h(endpoint),
        body_rec: record,
        lend,
        timeout: FOREVER,
    })
    .expect("call ABI") else {
        panic!("call outcome")
    };
    outcome
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let endpoint = rd::endpoint_create().unwrap();
    ENDPOINT.store(endpoint as usize, Ordering::Release);
    redoubt_abi::create_thread_1(server, 0).expect("server");
    let page = rd::page();

    // Recognized call errors retain even malformed raw lend arguments, before decoding h.
    for (addr, pages, lend) in [(0, 0, 0), (0, 1, 1), (page, 0, 1), (page, 1, 1)] {
        let regs = rd::raw_registers([rd::Number::Call as usize, 0, 0, addr, pages, 0, 0, 0]);
        assert_eq!(regs, [Error::BadHandle as usize, lend, 0, 0, 0, 0, 0, 0]);
    }
    write_body(page, rd::body([ECHO, 0, 0, 0]));
    rd::set_flags(page, rd::PAGE_SIZE, rd::MemFlags::READ).unwrap();
    for handle in [endpoint, u32::MAX] {
        assert_eq!(
            raw_call(handle, page, None),
            CallOutcome {
                status: Err(Error::InvalidArgument),
                lend: LendDisposition::None,
                reply_present: false,
            }
        );
    }
    let (mmio, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    assert_eq!(raw_call(endpoint, mmio, None).status, Err(Error::InvalidArgument));
    assert_eq!(TAKEN.load(Ordering::Acquire), 0, "invalid records were never delivered");
    rd::set_flags(page, rd::PAGE_SIZE, rd::rw()).unwrap();
    log!(logger, "IPC1 initial readonly/MMIO records refused before delivery");

    // Restore the lend before writing its overlapping output record.
    let out = raw_call(endpoint, page, rd::pages(page, 1));
    assert_eq!(out, CallOutcome { status: Ok(()), lend: LendDisposition::Returned, reply_present: true });
    assert_eq!(rd::peek(page), 42);
    assert_eq!(replied(1), ReplyOutcome { delivered: true, installed: 0 });
    log!(logger, "IPC1 record inside returned lend committed");

    // Timeout zero on an endpoint nobody serves deterministically cancels before receipt.
    let silent = rd::endpoint_create().unwrap();
    let (out, reply) = rd::call_outcome(silent, &rd::body([0; 4]), rd::pages(page, 1), 0).unwrap();
    assert_eq!(
        out,
        CallOutcome { status: Err(Error::Timeout), lend: LendDisposition::Returned, reply_present: false }
    );
    assert!(reply.is_none());
    assert_eq!(rd::peek(page), 42);
    rd::close(silent).unwrap();

    // Fill the first handle-table page, using copies of one endpoint (no new objects). A
    // reply's first newly installed handle must then allocate a second table page; failed
    // output must release that page again, along with every installed slot.
    let mut filler = [0u32; rd::MAX_HANDLES];
    let mut count = 0;
    while rd::first_free() <= 128 {
        filler[count] = rd::mint_from_handle(endpoint, 99, None).unwrap();
        count += 1;
    }
    for (iteration, op) in [READ_ONLY, UNMAP, REMAP].into_iter().enumerate() {
        let record = rd::page();
        write_body(record, rd::body([op, record, 0, 0]));
        let before = rd::usage(rd::SYSTEM).unwrap().pages_usage;
        let out = raw_call(endpoint, record, rd::pages(page, 1));
        assert_eq!(
            out,
            CallOutcome {
                status: Err(Error::InvalidArgument),
                lend: LendDisposition::Returned,
                reply_present: false
            }
        );
        assert_eq!(replied(2 + iteration), ReplyOutcome { delivered: false, installed: 0 });
        assert_eq!(rd::first_free(), 129, "all newly installed handles rolled back");
        assert_eq!(
            rd::usage(rd::SYSTEM).unwrap().pages_usage,
            before - u64::from(op == UNMAP),
            "reply/table/lend charges restored"
        );
        assert_eq!(rd::peek(page), 42, "lend returned independently of absent output");
        if op == REMAP {
            assert_eq!(rd::peek(record), 0xface, "replacement was not overwritten");
        }
        if op != UNMAP {
            rd::unmap(record, rd::PAGE_SIZE).unwrap();
        }
    }
    log!(logger, "IPC1 late readonly/unmap/remap discarded; handles and table pages rolled back");

    // Exactly one reply handle fits: keep its identity and all words on OutOfMemory.
    let mut next = rd::first_free();
    while next < rd::MAX_HANDLES as u32 {
        filler[count] = rd::mint_from_handle(endpoint, 99, None).unwrap();
        assert_eq!(filler[count], next);
        next += 1;
        count += 1;
    }
    let (out, reply) = rd::call_outcome(endpoint, &rd::body([HANDLES, 0, 0, 0]), None, FOREVER).unwrap();
    assert_eq!(
        out,
        CallOutcome { status: Err(Error::OutOfMemory), lend: LendDisposition::None, reply_present: true }
    );
    let reply = reply.unwrap();
    assert_eq!(reply.words, [42, 43, 0, 0]);
    assert_eq!(reply.handles.as_slice(), &[Some(rd::h(rd::MAX_HANDLES as u32)), None]);
    assert_eq!(replied(5), ReplyOutcome { delivered: true, installed: 1 });
    rd::close(rd::MAX_HANDLES as u32).unwrap();
    // Still only one free slot: output failure overrides attempted partial OutOfMemory,
    // and rolls back the handle that did fit without disturbing any pre-existing handle.
    let record = rd::page();
    write_body(record, rd::body([READ_ONLY, record, 0, 0]));
    let before = rd::usage(rd::SYSTEM).unwrap().pages_usage;
    let out = raw_call(endpoint, record, rd::pages(page, 1));
    assert_eq!(
        out,
        CallOutcome {
            status: Err(Error::InvalidArgument),
            lend: LendDisposition::Returned,
            reply_present: false
        }
    );
    assert_eq!(replied(6), ReplyOutcome { delivered: false, installed: 0 });
    assert_eq!(rd::first_free(), rd::MAX_HANDLES as u32);
    assert_eq!(rd::usage(rd::SYSTEM).unwrap().pages_usage, before);
    rd::unmap(record, rd::PAGE_SIZE).unwrap();
    for handle in &filler[..count] {
        rd::close(*handle).unwrap();
    }
    log!(logger, "IPC1 partial OOM reply preserves words, slots and installed mask");

    let before = rd::usage(rd::SYSTEM).unwrap().pages_usage;
    let (out, _) =
        rd::call_outcome(endpoint, &rd::body([LOAN_PROTECTION, page, 0, 0]), rd::pages(page, 1), FOREVER)
            .unwrap();
    assert_eq!(out, CallOutcome { status: Ok(()), lend: LendDisposition::Returned, reply_present: true });
    assert_eq!(replied(7), ReplyOutcome { delivered: true, installed: 0 });
    assert_eq!(rd::peek(page), 0xbeef, "same physical frame returned");
    assert_eq!(rd::usage(rd::SYSTEM).unwrap().pages_usage, before);
    log!(logger, "IPC1 both loan aliases protected; frame and charges preserved");

    // Destroying the request's stamp after receipt abandons it; the late reply is discarded.
    let before_abandon = rd::usage(rd::SYSTEM).unwrap().pages_usage;
    let scope = rd::create(rd::SYSTEM, &rd::spec(0, 0, 0)).unwrap();
    let stamped = rd::mint_from_handle(endpoint, 7, Some(scope)).unwrap();
    let (out, reply) =
        rd::call_outcome(stamped, &rd::body([REVOKE, scope as usize, 0, 0]), rd::pages(page, 1), FOREVER)
            .unwrap();
    assert_eq!(
        out,
        CallOutcome { status: Err(Error::Dead), lend: LendDisposition::Consumed, reply_present: false }
    );
    assert!(reply.is_none());
    assert_eq!(replied(8), ReplyOutcome { delivered: false, installed: 0 });
    assert_eq!(rd::usage(rd::SYSTEM).unwrap().pages_usage + 1, before_abandon);
    log!(logger, "IPC1 taken revocation consumed lend; server observed discard");

    let returned = rd::page();
    rd::poke(returned, 0xfeed);
    let (out, reply) =
        rd::call_outcome(endpoint, &rd::body([DIE, 0, 0, 0]), rd::pages(returned, 1), FOREVER).unwrap();
    assert_eq!(
        out,
        CallOutcome { status: Err(Error::Dead), lend: LendDisposition::Returned, reply_present: false }
    );
    assert!(reply.is_none());
    assert_eq!(rd::peek(returned), 0xfeed);
    log!(logger, "IPC1 server death returned lend without reply");
    log!(logger, "IPC1 KERNEL OUTCOMES PASSED");
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "IPC1 FAILED: {}", info);
    test_programs::park()
}
