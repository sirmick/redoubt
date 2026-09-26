//! Hostile process arguments and creator accounting, checked by a trusted parent.
//! Only the loader's parent holds the UART and Reset handles. Children are trusted probes: the
//! parent observes kernel errors, usage, delivered handle revocation and exit notices, then
//! resets.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Call, Cause, Error, MemFlags, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

const WAIT: u64 = 2_000_000;
const DEST: usize = 0x0800_0000;
const ACCOUNT: u64 = 47;
static CONSOLE: AtomicUsize = AtomicUsize::new(0);
static LATE_RECORD: AtomicUsize = AtomicUsize::new(0);
static LATE_BUDGET: AtomicUsize = AtomicUsize::new(0);

extern "C" fn invalidate_notice_record(_: usize) -> ! {
    // A single-hart kernel schedules this sibling only after the parent blocks in Receive:
    // thread_create returns to the parent, which performs no intervening yielding call.
    // Thus initial output validation already succeeded before this unmap executes.
    rd::unmap(LATE_RECORD.load(Ordering::Acquire), rd::PAGE_SIZE).unwrap();
    rd::destroy(LATE_BUDGET.load(Ordering::Acquire) as u32).unwrap();
    rd::thread_exit().unwrap();
    panic!("thread_exit returned")
}

struct Checker(MmioSerialPort);
impl Checker {
    fn check(&mut self, ok: bool, label: &str) {
        writeln!(self.0, "[proc-attack] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
        assert!(ok, "{}", label);
    }
}

fn receive(endpoint: u32) -> rd::Message {
    match rd::receive(Some(endpoint), WAIT, 0).expect("probe message") {
        Received::Message(m) => m,
        _ => panic!("expected probe message"),
    }
}
fn notice(endpoint: u32, cause: Cause, code: u32) -> u32 {
    match rd::receive(Some(endpoint), WAIT, 0).expect("kernel notice") {
        Received::Exit(n) => {
            assert_ne!(n.pid, 0);
            assert_eq!(n.cause, cause);
            assert_eq!(n.code, code);
            assert_eq!(n.blamed_account, 0);
            assert!(n.blamed_labels.as_slice().is_empty());
            n.pid
        }
        _ => panic!("expected exit notice"),
    }
}
fn handle(m: &rd::Message, index: usize) -> u32 {
    m.body.handles.as_slice()[index].expect("delivered handle").index()
}
fn sleep() -> ! {
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}
extern "C" fn sleeper(_: usize) -> ! { sleep() }
extern "C" fn exiter(_: usize) -> ! { rd::process_exit(19) }
extern "C" fn account_probe(_: usize) -> ! {
    let mut labelled = rd::spec(0, 0, 0);
    labelled.labels.push(99).unwrap();
    assert_eq!(rd::create(2, &labelled), Err(Error::ClassDenied));
    rd::send(1, &rd::body([0; rd::WORDS]), None, WAIT).expect("account observation");
    rd::process_exit(0)
}

extern "C" fn destroy_creator(_: usize) -> ! {
    // Destroying this scope also frees this process's creator-paid object: this syscall
    // must never return to the now-dead caller, though its execution budget survives.
    rd::destroy(1).unwrap();
    panic!("destroying creator returned to a dead process")
}

extern "C" fn external_launcher(_: usize) -> ! {
    spawn::spawn(&spawn::image(), 1, 3, destroy_creator as *const () as usize, &[], &[2])
        .expect("external child with creator scope grant");
    sleep()
}

// A user-class trusted probe creates objects using authority granted by the checker. The
// external parent survives this probe's budget: only the stamp may revoke its budget handle.
extern "C" fn stamps_probe(_: usize) -> ! {
    let mut labelled = rd::spec(0, 0, 0);
    labelled.labels.push(99).unwrap();
    assert_eq!(rd::create(2, &labelled), Err(Error::ClassDenied));
    // Its own user budget cannot read the labelled sibling, despite holding its handle.
    assert_eq!(rd::usage(5), Err(Error::LabelDenied));
    let mut spec = rd::spec(140, 2, 5);
    spec.account = 999; // R8 must preserve the nonzero parent's ACCOUNT instead.
    let budget = rd::create(2, &spec).expect("inherited budget");
    let endpoint = rd::endpoint_create().expect("probe endpoint");
    let child = rd::process_create(3, 4).expect("external child");
    spawn::spawn(&spawn::image(), budget, 4, account_probe as *const () as usize, &[], &[1, budget])
        .expect("account child");
    rd::call(1, &rd::body_with([0; rd::WORDS], &[budget, endpoint, child]), None, WAIT)
        .expect("checker acknowledgement");
    sleep()
}

// Reserve all but a few of this creator's pages, then repeatedly create into independently
// funded execution budgets. The checker kills each execution budget and retains its notice.
extern "C" fn exhaustion_probe(_: usize) -> ! {
    let free = rd::free(2);
    assert!(free > 12);
    rd::create(2, &rd::spec(free - 9, 0, 0)).expect("bounded creator reserve");
    let mut request = rd::body([0; rd::WORDS]);
    loop {
        let reply = rd::call(1, &request, None, WAIT).expect("next execution budget");
        let target = reply.handles.as_slice()[0].expect("target budget").index();
        let result = rd::process_create(target, 3);
        // Avoid accumulating granted execution-budget handles in the probe.
        rd::close(target).expect("close grant");
        request = match result {
            Ok(process) => {
                // Closing this last process handle cannot free its still-pending notice.
                rd::close(process).expect("close process handle");
                rd::body([1, 0, 0, 0])
            }
            Err(error) => rd::body([2, error as usize, 0, 0]),
        };
    }
}

#[inline(never)]
fn warm_stack() {
    let mut scratch = [0u8; 16384];
    for i in (0..scratch.len()).step_by(512) {
        // SAFETY: an in-bounds write; volatile faults in the checker's comparison stack.
        unsafe { core::ptr::write_volatile(&mut scratch[i], 1) };
    }
}

fn arguments(c: &mut Checker, image: &spawn::Image) {
    let exit = rd::endpoint_create().unwrap();
    let zero = rd::create(rd::SYSTEM, &rd::spec(32, 1, 0)).unwrap();
    c.check(rd::process_create(zero, exit) == Err(Error::InvalidArgument), "weight zero refused");
    rd::destroy(zero).unwrap();
    let budget = rd::create(rd::SYSTEM, &rd::spec(200, 2, 10)).unwrap();
    let badged = rd::mint_from_handle(exit, 1, None).unwrap();
    c.check(rd::process_create(budget, badged) == Err(Error::NotPermitted), "badged exit refused");
    rd::close(badged).unwrap();
    let process = rd::process_create(budget, exit).unwrap();
    let src = rd::map_anon(2 * rd::PAGE_SIZE, rd::rw()).unwrap();
    rd::poke(src, 0x1122);
    rd::poke(src + rd::PAGE_SIZE, 0x3344);
    rd::unmap(src + rd::PAGE_SIZE, rd::PAGE_SIZE).unwrap();
    let before = rd::usage(budget).unwrap();
    for (source, dst, len, flags) in [
        (src, DEST, rd::PAGE_SIZE, rd::rw() | MemFlags::EXECUTE),
        (src + 1, DEST, rd::PAGE_SIZE, rd::rw()),
        (src, DEST + 1, rd::PAGE_SIZE, rd::rw()),
        (src, DEST, rd::PAGE_SIZE - 1, rd::rw()),
        (src, DEST, 0, rd::rw()),
        (0, DEST, rd::PAGE_SIZE, rd::rw()),
        (src, DEST, 2 * rd::PAGE_SIZE, rd::rw()),
        (src, usize::MAX & !(rd::PAGE_SIZE - 1), 2 * rd::PAGE_SIZE, rd::rw()),
    ] {
        assert_eq!(rd::process_map(process, source, dst, len, flags), Err(Error::InvalidArgument));
        assert_eq!(rd::usage(budget), Ok(before));
        assert_eq!(rd::peek(src), 0x1122);
    }
    c.check(true, "hostile mapping ranges and W+X refused without moving source");
    rd::process_map(process, src, DEST, rd::PAGE_SIZE, rd::rw()).unwrap();
    let second = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap();
    rd::poke(second, 0x5566);
    let occupied = rd::usage(budget).unwrap();
    assert_eq!(rd::process_map(process, second, DEST, rd::PAGE_SIZE, rd::rw()), Err(Error::InvalidArgument));
    assert_eq!(rd::usage(budget), Ok(occupied));
    assert_eq!(rd::peek(second), 0x5566);
    c.check(true, "occupied destination refused without losing caller page");

    let too_many = Call::ProcessStart {
        process: rd::h(process),
        entry: 0,
        sp: 0,
        arg: 0,
        handles_rec: 0,
        count: redoubt_sys::MAX_START_HANDLES as u32 + 1,
    };
    c.check(
        redoubt_sys::syscall(&too_many) == Err(Error::TooLarge),
        "oversized startup list refused before record access",
    );
    let invalid_handle = [0u64];
    let bad_handle = Call::ProcessStart {
        process: rd::h(process),
        entry: 0,
        sp: 0,
        arg: 0,
        handles_rec: invalid_handle.as_ptr() as usize,
        count: 1,
    };
    assert_eq!(redoubt_sys::syscall(&bad_handle), Err(Error::BadHandle));
    let invalid_record =
        Call::ProcessStart { process: rd::h(process), entry: 0, sp: 0, arg: 0, handles_rec: 1, count: 1 };
    assert_eq!(redoubt_sys::syscall(&invalid_record), Err(Error::InvalidArgument));
    let wrong_object_record =
        Call::ProcessStart { process: rd::h(exit), entry: 0, sp: 0, arg: 0, handles_rec: 1, count: 1 };
    assert_eq!(redoubt_sys::syscall(&wrong_object_record), Err(Error::InvalidArgument));
    let wrong_object_bad_slot = Call::ProcessStart {
        process: rd::h(exit),
        entry: 0,
        sp: 0,
        arg: 0,
        handles_rec: invalid_handle.as_ptr() as usize,
        count: 1,
    };
    assert_eq!(redoubt_sys::syscall(&wrong_object_bad_slot), Err(Error::BadHandle));
    assert_eq!(rd::usage(budget), Ok(occupied));
    c.check(true, "invalid startup records and handles leave process unchanged");
    rd::unmap(second, rd::PAGE_SIZE).unwrap();
    rd::destroy(budget).unwrap();
    notice(exit, Cause::Killed, 0);

    let budget = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).unwrap();
    let child = spawn::spawn(image, budget, exit, sleeper as *const () as usize, &[], &[]).unwrap();
    let src = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap();
    rd::poke(src, 7);
    assert_eq!(rd::process_map(child.process, src, DEST, rd::PAGE_SIZE, rd::rw()), Err(Error::NotPermitted));
    assert_eq!(
        rd::process_map(child.process, src, spawn::IMAGE_BASE, rd::PAGE_SIZE, rd::rw()),
        Err(Error::InvalidArgument)
    );
    assert_eq!(
        rd::process_start(child.process, sleeper as *const () as usize, spawn::STACK_TOP - 16, 0, &[]),
        Err(Error::NotPermitted)
    );
    assert_eq!(rd::peek(src), 7);
    rd::unmap(src, rd::PAGE_SIZE).unwrap();
    c.check(true, "started process rejects mapping and restart");
    rd::destroy(budget).unwrap();
    notice(exit, Cause::Killed, 0);
    rd::close(exit).unwrap();
}

fn pending_record(c: &mut Checker, image: &spawn::Image) {
    let exit = rd::endpoint_create().unwrap();
    let base = rd::usage(rd::SYSTEM).unwrap();
    let budget = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).unwrap();
    let carved = rd::usage(rd::SYSTEM).unwrap();
    assert_eq!(carved.pages_usage, base.pages_usage + 201);
    let process = rd::process_create(budget, exit).unwrap();
    assert_eq!(rd::usage(rd::SYSTEM).unwrap().pages_usage, carved.pages_usage + 1);
    let execution = rd::usage(budget).unwrap();
    assert_eq!(execution.processes_usage, 1);
    assert!(execution.pages_usage > 0);
    rd::destroy(budget).unwrap(); // Notice is pending synchronously; no timing assumption.
    let pending = rd::Usage { pages_usage: base.pages_usage + 1, ..base };
    assert_eq!(rd::usage(rd::SYSTEM), Ok(pending));
    let bad_record = Call::Receive { from: Some(rd::h(exit)), timeout: 0, max_transfer: 0, received_rec: 0 };
    assert_eq!(redoubt_sys::syscall(&bad_record), Err(Error::InvalidArgument));
    // A pending process handle remains an existing process object, not a dead handle.
    assert_eq!(rd::usage(process), Err(Error::WrongObject));
    assert_eq!(rd::usage(rd::SYSTEM), Ok(pending));
    notice(exit, Cause::Killed, 0);
    assert_eq!(rd::usage(process), Err(Error::BadHandle));
    assert_eq!(rd::usage(rd::SYSTEM), Ok(base));
    c.check(true, "bad receive output preserves pending notice and process object");
    // A naturally exiting process frees all execution charges before its object is received.
    let budget = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).unwrap();
    let before = rd::usage(budget).unwrap();
    spawn::spawn(image, budget, exit, exiter as *const () as usize, &[], &[]).unwrap();
    notice(exit, Cause::Exited, 19);
    assert_eq!(rd::usage(budget), Ok(before));
    rd::destroy(budget).unwrap();
    rd::close(exit).unwrap();
    c.check(true, "exit restores exact execution budget usage");
}

fn late_record(c: &mut Checker) {
    let exit = rd::endpoint_create().unwrap();
    let budget = rd::create(rd::SYSTEM, &rd::spec(32, 1, 5)).unwrap();
    let process = rd::process_create(budget, exit).unwrap();
    let record = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap();
    rd::poke(record, 0);
    let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).unwrap();
    LATE_RECORD.store(record, Ordering::Release);
    LATE_BUDGET.store(budget as usize, Ordering::Release);
    rd::thread_create(invalidate_notice_record as *const () as usize, stack + 4 * rd::PAGE_SIZE - 16, 0).unwrap();
    let receive =
        Call::Receive { from: Some(rd::h(exit)), timeout: WAIT, max_transfer: 0, received_rec: record };
    assert_eq!(redoubt_sys::syscall(&receive), Err(Error::InvalidArgument));
    assert_eq!(rd::usage(process), Err(Error::WrongObject));
    notice(exit, Cause::Killed, 0);
    assert_eq!(rd::usage(process), Err(Error::BadHandle));
    rd::close(exit).unwrap();
    c.check(true, "late receive output failure preserves exit notice and process object");
}

fn destroy_own_creator(c: &mut Checker, image: &spawn::Image) {
    for runs_outside in [false, true] {
        let exit = rd::endpoint_create().unwrap();
        let grandchild_exit = rd::endpoint_create().unwrap();
        let scope = rd::create(rd::SYSTEM, &rd::spec(240, 2, 10)).unwrap();
        let external = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).unwrap();
        let baseline = rd::usage(external).unwrap();
        spawn::spawn(
            image,
            scope,
            exit,
            external_launcher as *const () as usize,
            &[],
            &[if runs_outside { external } else { scope }, scope, grandchild_exit],
        )
        .unwrap();
        // Block on the external endpoint before the launcher/grandchild runs. A premature
        // notice pump during creator teardown must not expose a notice that should be dropped.
        assert_eq!(rd::receive(Some(grandchild_exit), 200_000, 0), Err(Error::Timeout));
        notice(exit, Cause::Killed, 0);
        assert_eq!(rd::usage(scope), Err(Error::BadHandle));
        assert_eq!(rd::usage(external), Ok(baseline));
        assert_eq!(rd::receive(Some(grandchild_exit), 0, 0), Err(Error::Timeout));
        rd::destroy(external).unwrap();
        rd::close(exit).unwrap();
        rd::close(grandchild_exit).unwrap();
    }
    c.check(true, "destroying own creator scope kills external caller without notice");
    c.check(true, "creator destruction suppresses notices even for a blocked external receiver");
}

fn stamps(c: &mut Checker, image: &spawn::Image) {
    let work = rd::endpoint_create().unwrap();
    let exit = rd::endpoint_create().unwrap();
    let child_exit = rd::endpoint_create().unwrap();
    let mut spec = rd::spec(500, 3, 30);
    spec.account = ACCOUNT;
    let external = rd::create(rd::USERS, &spec).unwrap();
    let target = rd::create(rd::SYSTEM, &rd::spec(32, 1, 5)).unwrap();
    let mut labelled = rd::spec(1, 0, 0);
    labelled.labels.push(99).unwrap();
    let hidden = rd::create(rd::USERS, &labelled).unwrap();
    let owner = rd::create(rd::USERS, &rd::spec(200, 1, 10)).unwrap();
    let before = rd::usage(external).unwrap();
    spawn::spawn(
        image,
        owner,
        exit,
        stamps_probe as *const () as usize,
        &[],
        &[work, external, target, child_exit, hidden],
    )
    .unwrap();
    // Account child and object message may arrive in either order; both are kernel messages.
    let mut objects = None;
    let mut inherited = false;
    for _ in 0..2 {
        let m = receive(work);
        if m.body.handles.as_slice().is_empty() {
            assert_eq!(m.account, ACCOUNT);
            inherited = true;
        } else {
            assert_eq!(m.body.handles.as_slice().len(), 3);
            objects = Some(m);
        }
    }
    assert!(inherited);
    let objects = objects.unwrap();
    let budget_handle = handle(&objects, 0);
    let endpoint_handle = handle(&objects, 1);
    let process_handle = handle(&objects, 2);
    assert_eq!(rd::usage(budget_handle).unwrap().pages_limit, 140);
    assert_eq!(rd::usage(endpoint_handle), Err(Error::WrongObject));
    assert_eq!(rd::usage(process_handle), Err(Error::WrongObject));
    rd::reply(objects.msg_id.get(), &rd::body([0; rd::WORDS])).unwrap();
    notice(child_exit, Cause::Exited, 0);
    c.check(true, "R8 account inheritance observed in kernel message metadata");
    rd::destroy(owner).unwrap();
    notice(exit, Cause::Killed, 0);
    for h in [budget_handle, endpoint_handle, process_handle] {
        assert_eq!(rd::close(h), Err(Error::BadHandle));
    }
    // The external budget object survives; only its handle stamped by owner was revoked.
    let after = rd::usage(external).unwrap();
    assert_eq!(after.pages_usage, before.pages_usage + 141);
    assert_eq!(after.processes_usage, before.processes_usage + 2);
    assert_eq!(after.weight_carved, before.weight_carved + 5);
    assert_eq!(rd::usage(target).unwrap().processes_usage, 0);
    assert_eq!(rd::receive(Some(child_exit), 0, 0), Err(Error::Timeout));
    c.check(true, "R9 all creation stamps revoke while external budget survives");
    c.check(true, "I8 user class refuses labels and labelled sibling usage");
    for budget in [external, target, hidden] {
        rd::destroy(budget).unwrap();
    }
    for endpoint in [work, exit, child_exit] {
        rd::close(endpoint).unwrap();
    }
}

fn exhaustion(c: &mut Checker, image: &spawn::Image) {
    let work = rd::endpoint_create().unwrap();
    let exit = rd::endpoint_create().unwrap();
    let pending = rd::endpoint_create().unwrap();
    let owner = rd::create(rd::SYSTEM, &rd::spec(200, 1, 20)).unwrap();
    spawn::spawn(image, owner, exit, exhaustion_probe as *const () as usize, &[], &[work, owner, pending])
        .unwrap();
    let mut request = receive(work);
    let initial = rd::usage(owner).unwrap();
    let room = initial.pages_limit - initial.pages_usage;
    assert!((1..=8).contains(&room));
    let mut count = 0;
    loop {
        let target = rd::create(rd::SYSTEM, &rd::spec(32, 1, 5)).unwrap();
        rd::reply(request.msg_id.get(), &rd::body_with([0; rd::WORDS], &[target])).unwrap();
        request = receive(work);
        rd::destroy(target).unwrap();
        if request.body.words[0] == 2 {
            assert_eq!(request.body.words[1], Error::OutOfMemory as usize);
            break;
        }
        assert_eq!(request.body.words[0], 1);
        count += 1;
        assert!(count <= room);
        let usage = rd::usage(owner).unwrap();
        assert_eq!(usage.pages_usage, initial.pages_usage + count);
        assert_eq!(usage.processes_usage, initial.processes_usage);
    }
    assert_eq!(count, room);
    assert_eq!(rd::free(owner), 0);
    c.check(true, "unreceived notices exhaust only their creator budget");
    // All these notices were pending at once. Compare every PID, including the first,
    // before creating another process (which may legitimately reuse a received PID).
    let mut pids = [0u32; 8];
    for i in 0..count as usize {
        let pid = notice(pending, Cause::Killed, 0);
        assert!(!pids[..i].contains(&pid));
        pids[i] = pid;
        assert_eq!(rd::free(owner), i as u64 + 1);
    }
    c.check(true, "PIDs remain distinct while their notices are pending");
    assert_eq!(rd::usage(owner), Ok(initial));
    let target = rd::create(rd::SYSTEM, &rd::spec(32, 1, 5)).unwrap();
    rd::reply(request.msg_id.get(), &rd::body_with([0; rd::WORDS], &[target])).unwrap();
    let last = receive(work);
    assert_eq!(last.body.words[0], 1);
    rd::destroy(target).unwrap();
    assert_eq!(rd::free(owner), room - 1);
    notice(pending, Cause::Killed, 0);
    assert_eq!(rd::usage(owner), Ok(initial));
    c.check(true, "receiving notices refunds exact creator pages and permits reuse");
    rd::destroy(owner).unwrap();
    rd::reply(last.msg_id.get(), &rd::body([0; rd::WORDS])).unwrap();
    notice(exit, Cause::Killed, 0);
    assert_eq!(rd::receive(Some(pending), 0, 0), Err(Error::Timeout));
    for endpoint in [work, exit, pending] {
        rd::close(endpoint).unwrap();
    }
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut c = Checker(unsafe { MmioSerialPort::new(uart) });
    c.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(c.0).ok();
    warm_stack();
    let image = spawn::image();
    arguments(&mut c, &image);
    pending_record(&mut c, &image);
    late_record(&mut c);
    destroy_own_creator(&mut c, &image);
    stamps(&mut c, &image);
    exhaustion(&mut c, &image);
    writeln!(c.0, "[proc-attack] PROCESS ATTACK TEST PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    sleep()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only the trusted parent initializes CONSOLE, to its granted UART page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[proc-attack] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
