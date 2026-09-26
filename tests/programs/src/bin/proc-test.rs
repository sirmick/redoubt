//! Process creation and exit (WP-K4): `process_create`, `process_map`, `process_start` with its
//! `arg`, `thread_create`, `thread_exit`, `process_exit`, and the exit notices they produce with
//! their cause, code, blamed account and blamed labels (KERNEL-SPEC.md, Process, Messages; R10).
//!
//! It runs as the bundle's first program, so it holds `root`, `system` and `users` and every
//! device object, and it prints through the console it maps itself -- exactly as `device-test`
//! does, and for the same reason: there is no `log-server` here because only one process can own
//! the UART.
//!
//! **Its children cannot print.** A child is a copy of this image (`test_programs::spawn`), so it
//! holds the code, but its handle table holds only what `process_start` gave it, so `map_device`
//! of the UART refuses it. Every
//! line below is therefore this process's, and what it says about a child is what the *kernel*
//! told it in an exit notice: a cause, a code, an account and a label set none of which a child
//! can choose.
//!
//! An exit notice names the process by PID, and `process_create` never tells its caller the PID
//! it drew, so a parent cannot match a notice to a child by name. Each child here gets an exit
//! endpoint of its own, which is how a launcher tells them apart (reported).

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Error, ExitNotice, Received, ResetKind};
use test_programs::spawn::{self, Image};
use uart_16550::MmioSerialPort;

// --- What a child does, from byte 0 of its startup page -----------------------------------------
/// Exit with the code in byte 1.
const R_EXIT: u8 = 1;
/// Fault at once, with no call ever taken: nobody is blamed.
const R_FAULT: u8 = 2;
/// Never finish, so that something else must end it.
const R_SPIN: u8 = 3;
/// Call on handle 1 with byte 1 as word 0, then exit with 100 + the error's code.
const R_CALL: u8 = 4;
/// Send on handle 1, then exit 0.
const R_SEND: u8 = 5;
/// Receive one call on handle 1 (a receive right), then fault: the call is blamed.
const R_SERVE_FAULT: u8 = 6;
/// Receive one call on handle 1, then `process_exit`: `faulted`, blamed on that call (answer 55).
const R_EXIT_OPEN: u8 = 7;
/// Receive one call on handle 1, reply to it, then fault: nobody is blamed.
const R_REPLY_FAULT: u8 = 8;
/// Receive one message on handle 1 (a `send`), then fault: nobody is blamed.
const R_SEND_FAULT: u8 = 9;
/// Receive two calls on handle 1, `serve` the first again, then fault: the first is blamed.
const R_TWO_CALLS: u8 = 10;
/// Receive one call on handle 1 and hold it; a second thread faults: nobody is blamed.
const R_OTHER_THREAD: u8 = 11;
/// Check the startup block this program wrote and exit with a code that says so.
const R_STARTUP: u8 = 12;
/// Write to the startup page, which is read-only: a fault.
const R_STARTUP_WRITE: u8 = 13;
/// Create a grandchild in the budget handle 1 names, start it spinning, then spin.
const R_LAUNCHER: u8 = 14;
const R_TWO_EXIT: u8 = 15;
const R_OTHER_EXIT: u8 = 16;
const R_LAST_EXIT: u8 = 17;
const R_LAST_OPEN: u8 = 18;
const R_LAST_NO_CURRENT: u8 = 19;
const R_RETURN_EMPTY: u8 = 20;
const R_RETURN_OPEN: u8 = 21;

/// What `R_STARTUP` looks for, at offset 4 of the startup page.
const STARTUP_MAGIC: u32 = 0xc0ff_ee01;
/// `R_STARTUP`'s exit code when the block is where and what it should be.
const STARTUP_OK: u32 = 55;

/// The accounts the caller children's budgets carry, so that a blamed account is a number no
/// child could have chosen and no unblamed notice could show.
const ACCOUNT_A: u64 = 42;
const ACCOUNT_B: u64 = 43;
/// The label the caller children's budgets carry.
const LABEL: u64 = 7;

/// Long enough that nothing here ever waits it out on a working kernel, short enough that a
/// notice that never comes fails the case rather than hanging it.
const WAIT: u64 = 2_000_000;
/// Long enough for every child of a scenario to have started and made its call.
const BARRIER: u64 = 200_000;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[proc] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

// --- The child ---------------------------------------------------------------------------------

/// Everything a child does. `arg` is its startup page, mapped read-only by its parent, and
/// handle 1 (and sometimes 2) is what `process_start` put in its table.
fn child(arg: usize) -> ! {
    let role = spawn::startup_byte(arg, 0);
    let value = spawn::startup_byte(arg, 1);
    match role {
        R_EXIT => rd::process_exit(u32::from(value)),
        R_RETURN_EMPTY | R_RETURN_OPEN => {
            let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).unwrap();
            rd::thread_create(
                returning_worker as *const () as usize,
                stack + 4 * rd::PAGE_SIZE - 16,
                role as usize,
            )
            .unwrap();
            // No intervening yield: the initial thread exits before its worker runs in K4.
            rd::thread_exit().expect("initial thread exits");
            panic!("thread_exit returned");
        }
        R_LAST_EXIT => {
            rd::thread_exit().expect("final thread_exit");
            panic!("final thread_exit returned")
        }
        R_LAST_OPEN | R_LAST_NO_CURRENT => {
            take_call();
            if role == R_LAST_NO_CURRENT {
                // Completing a non-call receive clears the current call, but leaves the
                // previously taken call open: final-thread exit is still Faulted.
                assert_eq!(rd::receive(None, 0, 0), Err(Error::Timeout));
            }
            rd::thread_exit().expect("final thread_exit with open call");
            panic!("final thread_exit returned")
        }
        R_FAULT => fault(),
        R_CALL => {
            let body = rd::body([usize::from(value), 0, 0, 0]);
            let code = match rd::call(1, &body, None, WAIT) {
                Ok(_) => 0,
                Err(e) => 100 + e as u32,
            };
            rd::process_exit(code)
        }
        R_SEND => {
            let body = rd::body([usize::from(value), 0, 0, 0]);
            let code = match rd::send(1, &body, None, WAIT) {
                Ok(()) => 0,
                Err(e) => 100 + e as u32,
            };
            rd::process_exit(code)
        }
        R_SERVE_FAULT => {
            take_call();
            fault()
        }
        R_EXIT_OPEN => {
            take_call();
            rd::process_exit(u32::from(value))
        }
        R_REPLY_FAULT => {
            let id = take_call();
            rd::reply(id, &rd::body([0; rd::WORDS])).ok();
            fault()
        }
        R_SEND_FAULT => {
            rd::receive(Some(1), WAIT, 0).ok();
            fault()
        }
        R_TWO_CALLS | R_TWO_EXIT => {
            // Wait until both callers have queued, so that which call is taken first is R2's
            // decision and not a race: the lowest group in R2's order goes first, and the
            // groups here differ only by account, so the lower account is served first.
            rd::receive(None, BARRIER, 0).ok();
            let first = take_call();
            take_call();
            // Blame follows the *current* call, which `serve` moves back to the first one
            // (answer 82), not the one taken last.
            rd::serve(first).ok();
            if role == R_TWO_EXIT {
                rd::process_exit(27);
            }
            fault()
        }
        R_OTHER_THREAD | R_OTHER_EXIT => {
            take_call();
            // A thread of its own, with a stack of its own inside this one's: the faulting
            // thread has no current call, and the one that has is not the one that faults.
            let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).expect("a stack for the thread");
            let entry =
                if role == R_OTHER_EXIT { entry_of(exiting_process_thread) } else { entry_of(fault_thread) };
            rd::thread_create(entry, stack + 4 * rd::PAGE_SIZE - 16, 0).expect("thread_create");
            sleep()
        }
        R_STARTUP => {
            let magic = u32::from_le_bytes(core::array::from_fn(|i| spawn::startup_byte(arg, 4 + i)));
            let where_ok = arg == spawn::STARTUP_AT;
            rd::process_exit(if magic == STARTUP_MAGIC && where_ok { STARTUP_OK } else { 1 })
        }
        R_STARTUP_WRITE => {
            // SAFETY: not sound, and that is the point: the parent mapped this page read-only,
            // so the store must fault rather than take effect.
            unsafe { (arg as *mut u8).write_volatile(0xff) };
            rd::process_exit(0)
        }
        R_LAUNCHER => {
            let image = spawn::image();
            let exit = rd::endpoint_create().expect("an exit endpoint of its own");
            let mut block = [0u8; 8];
            block[0] = R_SPIN;
            spawn::spawn(&image, 1, exit, entry_of(child_entry), &block, &[]).expect("a grandchild");
            sleep()
        }
        // R_SPIN and anything else: never finish.
        _ => sleep(),
    }
}

/// Wait for ever without running: `receive` with no handle is the one call that sleeps
/// (KERNEL-SPEC.md, `receive`). A child that spun instead would keep the kernel out of its idle
/// branch, where the interim timeout sweep lives until WP-K5 arms a timer.
fn sleep() -> ! {
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}

/// Take one call on handle 1 and hold it open; its message id.
fn take_call() -> u64 {
    match rd::receive(Some(1), WAIT, 0) {
        Ok(Received::Message(m)) => m.msg_id.get(),
        // No call came. Fault anyway: the parent then sees a notice that blames nobody, which
        // fails its check loudly instead of leaving it waiting.
        _ => fault(),
    }
}

/// The address of an entry point, as `process_start` and `thread_create` take it.
fn entry_of(f: extern "C" fn(usize) -> !) -> usize { f as *const () as usize }

/// A store to address 0, which no process ever has mapped.
fn fault() -> ! {
    // SAFETY: not sound, and that is the point: this must trap, so that the kernel reports the
    // process `faulted` rather than the process deciding how it ended.
    unsafe { (0usize as *mut u64).write_volatile(1) };
    test_programs::park()
}

extern "C" fn returning_worker(role: usize) -> usize {
    if role == R_RETURN_OPEN as usize {
        take_call();
    }
    // A thread's return value is not a process exit code: the final return exits with zero.
    171
}

extern "C" fn exiting_process_thread(_: usize) -> ! { rd::process_exit(29) }

extern "C" fn fault_thread(_arg: usize) -> ! { fault() }

/// Where a child starts. It is this same image, so the address is the same in both processes.
extern "C" fn child_entry(arg: usize) -> ! { child(arg) }

// --- The parent --------------------------------------------------------------------------------

/// One child, from its startup block to the notice the kernel sends when it ends.
struct Run {
    exit: u32,
    process: u32,
}

struct Parent {
    image: Image,
}

impl Parent {
    /// Start a child in `budget` with `role` and `value` in its startup page, and `handles` in
    /// its slots 1..n, on an exit endpoint of its own.
    fn start(&self, budget: u32, role: u8, value: u8, handles: &[u32]) -> Result<Run, Error> {
        let exit = rd::endpoint_create()?;
        let mut block = [0u8; 8];
        block[0] = role;
        block[1] = value;
        block[4..].copy_from_slice(&STARTUP_MAGIC.to_le_bytes());
        let child = spawn::spawn(&self.image, budget, exit, entry_of(child_entry), &block, handles)?;
        Ok(Run { exit, process: child.process })
    }

    /// The exit notice of the child that reports to `exit`, or `None` if none arrives.
    fn notice(&self, run: &Run) -> Option<ExitNotice> {
        match rd::receive(Some(run.exit), WAIT, 0) {
            Ok(Received::Exit(notice)) => Some(notice),
            _ => None,
        }
    }
}

/// Whether a notice says exactly this: cause, code, blamed account and blamed labels.
fn is(notice: &Option<ExitNotice>, cause: Cause, code: u32, account: u64, labels: &[u64]) -> bool {
    notice.as_ref().is_some_and(|n| {
        n.pid != 0
            && n.cause == cause
            && n.code == code
            && n.blamed_account == account
            && n.blamed_labels.as_slice() == labels
    })
}

fn show(out: &mut Out, what: &str, notice: &Option<ExitNotice>) {
    match notice {
        None => say!(out, "[proc] {}: no notice", what),
        Some(n) => say!(
            out,
            "[proc] {}: pid {} cause {:?} code {} account {} labels {}",
            what,
            n.pid,
            n.cause,
            n.code,
            n.blamed_account,
            n.blamed_labels.as_slice().len()
        ),
    }
}

#[no_mangle]
pub extern "C" fn _start(arg: usize) -> ! {
    // A child is this same image started with a startup page; the loader's copy gets no `arg`.
    if arg != 0 {
        child(arg)
    }

    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    say!(out, "\n[proc] mapped the console");

    let parent = Parent { image: spawn::image() };
    say!(out, "[proc] image is {} pages", parent.image.pages());

    // Budgets for the caller children: class `user` (inherited from `users`), each with an
    // account of its own and the same label. Only a system-class creator may add labels, which
    // this process is; the children below are not, which the attack case checks.
    let mut spec_a = rd::spec(400, 4, 100);
    spec_a.account = ACCOUNT_A;
    spec_a.labels.push(LABEL).expect("a label");
    let budget_a = rd::create(rd::USERS, &spec_a).expect("a budget for the callers");
    let mut spec_b = rd::spec(400, 4, 100);
    spec_b.account = ACCOUNT_B;
    spec_b.labels.push(LABEL).expect("a label");
    let budget_b = rd::create(rd::USERS, &spec_b).expect("a second budget for the callers");
    let usage_a = rd::usage(budget_a).expect("budget_usage");
    check!(
        out,
        usage_a.processes_limit == 4 && usage_a.processes_usage == 0,
        "a child budget starts with no processes"
    );

    // --- The three causes ---------------------------------------------------------------
    let run = parent.start(rd::SYSTEM, R_EXIT, 7, &[]).expect("a child that exits");
    let notice = parent.notice(&run);
    show(&mut out, "exited", &notice);
    check!(out, is(&notice, Cause::Exited, 7, 0, &[]), "process_exit(7) -> exited, code 7, nobody blamed");
    // The object is gone with its notice, so the handle that named it is gone too (I1).
    check!(
        out,
        rd::usage(run.process) == Err(Error::BadHandle),
        "the process handle is closed once its notice is taken"
    );

    let run = parent.start(rd::SYSTEM, R_FAULT, 0, &[]).expect("a child that faults");
    let notice = parent.notice(&run);
    show(&mut out, "faulted", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 15, 0, &[]),
        "a store to a page it has not got -> faulted, nobody blamed"
    );

    // `killed`: a spinning child in a budget of its own, ended by destroying that budget. Its
    // process object is charged here, in `system`, so the notice survives the budget (R10).
    let doomed = rd::create(rd::SYSTEM, &rd::spec(200, 2, 50)).expect("a budget to destroy");
    let run = parent.start(doomed, R_SPIN, 0, &[]).expect("a child to kill");
    rd::destroy(doomed).expect("budget_destroy");
    let notice = parent.notice(&run);
    show(&mut out, "killed", &notice);
    check!(out, is(&notice, Cause::Killed, 0, 0, &[]), "budget_destroy -> killed, code 0, nobody blamed");

    let run = parent.start(rd::SYSTEM, R_LAST_EXIT, 0, &[]).expect("a final-thread exit");
    let notice = parent.notice(&run);
    check!(
        out,
        is(&notice, Cause::Exited, 0, 0, &[]),
        "the final thread exits its empty process with code zero"
    );

    let run = parent.start(rd::SYSTEM, R_RETURN_EMPTY, 0, &[]).expect("a returning final worker");
    let notice = parent.notice(&run);
    check!(
        out,
        is(&notice, Cause::Exited, 0, 0, &[]),
        "returning final worker exits its empty process with code zero"
    );

    // --- A labelled process's notice reaches a system-class reader ------------------------
    let run = parent.start(budget_a, R_EXIT, 9, &[]).expect("a labelled child");
    let notice = parent.notice(&run);
    show(&mut out, "labelled", &notice);
    check!(
        out,
        is(&notice, Cause::Exited, 9, 0, &[]),
        "a labelled process's notice reaches a system-class reader (R1)"
    );

    // --- Blame ----------------------------------------------------------------------------
    blame(&mut out, &parent, budget_a, budget_b);

    // --- The startup block ------------------------------------------------------------------
    let run = parent.start(rd::SYSTEM, R_STARTUP, 0, &[]).expect("a child with a startup block");
    let notice = parent.notice(&run);
    show(&mut out, "startup", &notice);
    check!(
        out,
        is(&notice, Cause::Exited, STARTUP_OK, 0, &[]),
        "a child finds its startup block through arg, at the address its parent chose"
    );

    let run = parent.start(rd::SYSTEM, R_STARTUP_WRITE, 0, &[]).expect("a child that writes it");
    let notice = parent.notice(&run);
    show(&mut out, "startup-write", &notice);
    check!(out, is(&notice, Cause::Faulted, 15, 0, &[]), "the startup page is read-only: writing it faults");

    // --- A creator's budget destroyed kills its children with no notice ---------------------
    no_notice(&mut out, &parent);

    // --- Threads ------------------------------------------------------------------------------
    threads(&mut out);

    say!(out, "[proc] PROC TEST PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    test_programs::park()
}

/// Blame (answers 37, 55, 82): a fault blames the sender of the faulting thread's current call,
/// and nothing else.
fn blame(out: &mut Out, parent: &Parent, budget_a: u32, budget_b: u32) {
    // One work endpoint per scenario, owned by this process (class `system`), so R1 lets the
    // labelled callers through and the label set in a notice is the *caller's*, not ours.
    // Each caller's own notice is awaited too, not just the server's: a caller that has not
    // yet run its own `process_exit` still holds its budget's process slot and its PID, and the
    // next scenario reuses `budget_a`/`budget_b` right away. Without this wait, reuse races the
    // caller's actual termination and can spuriously refuse the next scenario's start with
    // `OutOfProcesses`; taking the notice also frees the process object and its PID.
    let scenario = |role: u8, value: u8, callers: &[(u32, u8, u8)]| -> Option<ExitNotice> {
        let work = rd::endpoint_create().expect("a work endpoint");
        let server = parent.start(rd::SYSTEM, role, value, &[work]).expect("a server child");
        let mut caller_runs: [Option<Run>; 2] = [None, None];
        assert!(callers.len() <= caller_runs.len(), "a scenario has at most two callers");
        for (slot, (budget, caller_role, tag)) in caller_runs.iter_mut().zip(callers) {
            let badged = rd::mint_from_handle(work, 1 + u64::from(*tag), None).expect("mint");
            *slot = Some(parent.start(*budget, *caller_role, *tag, &[badged]).expect("a caller child"));
        }
        let notice = parent.notice(&server);
        for run in caller_runs.iter().flatten() {
            parent.notice(run).expect("each caller's exit notice");
        }
        notice
    };

    let notice = scenario(R_SERVE_FAULT, 0, &[(budget_a, R_CALL, 1)]);
    show(out, "blame-server-fault", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 15, ACCOUNT_A, &[LABEL]),
        "a server that faults blames the account and labels of its current call's sender"
    );

    let notice = scenario(R_EXIT_OPEN, 9, &[(budget_a, R_CALL, 1)]);
    show(out, "blame-exit-open", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 9, ACCOUNT_A, &[LABEL]),
        "process_exit holding an open call is faulted, blamed on that call (answer 55)"
    );

    // Two callers with different accounts: after `serve`, the call `serve` named is blamed, not
    // the one taken last (answer 37 as the spec now states it, answer 82).
    let notice = scenario(R_TWO_CALLS, 0, &[(budget_a, R_CALL, 1), (budget_b, R_CALL, 2)]);
    show(out, "blame-serve", &notice);
    let served_first = is(&notice, Cause::Faulted, 15, ACCOUNT_A, &[LABEL]);
    check!(
        out,
        served_first,
        "a thread holding two callers' calls blames the one `serve` named, not the other"
    );

    let notice = scenario(R_REPLY_FAULT, 0, &[(budget_a, R_CALL, 1)]);
    show(out, "blame-after-reply", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 15, 0, &[]),
        "a crash after replying blames nobody: the reply cleared the current call"
    );

    let notice = scenario(R_SEND_FAULT, 0, &[(budget_a, R_SEND, 1)]);
    show(out, "blame-send", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 15, 0, &[]),
        "a send is never an open call, so a crash after taking one blames nobody"
    );

    let notice = scenario(R_OTHER_THREAD, 0, &[(budget_a, R_CALL, 1)]);
    show(out, "blame-other-thread", &notice);
    check!(
        out,
        is(&notice, Cause::Faulted, 15, 0, &[]),
        "a thread with no current call blames nobody, though another thread holds one"
    );
    let notice = scenario(R_TWO_EXIT, 0, &[(budget_a, R_CALL, 1), (budget_b, R_CALL, 2)]);
    check!(
        out,
        is(&notice, Cause::Faulted, 27, ACCOUNT_A, &[LABEL]),
        "process_exit with multiple calls blames only the call selected by serve"
    );
    let notice = scenario(R_OTHER_EXIT, 0, &[(budget_a, R_CALL, 1)]);
    check!(
        out,
        is(&notice, Cause::Faulted, 29, 0, &[]),
        "process_exit without a current call blames nobody while a sibling holds calls"
    );
    let notice = scenario(R_LAST_OPEN, 0, &[(budget_a, R_CALL, 1)]);
    check!(
        out,
        is(&notice, Cause::Faulted, 0, ACCOUNT_A, &[LABEL]),
        "final thread_exit snapshots current-call blame before cleanup"
    );
    let notice = scenario(R_LAST_NO_CURRENT, 0, &[(budget_a, R_CALL, 1)]);
    check!(
        out,
        is(&notice, Cause::Faulted, 0, 0, &[]),
        "final thread_exit with open calls and no current call blames nobody"
    );
    let notice = scenario(R_RETURN_OPEN, 0, &[(budget_a, R_CALL, 1)]);
    check!(
        out,
        is(&notice, Cause::Faulted, 0, ACCOUNT_A, &[LABEL]),
        "returning final worker snapshots current-call blame before cleanup"
    );
}

/// R10: destroying the creator's budget frees its children's process objects, killing them
/// first, and then there is no notice at all.
fn no_notice(out: &mut Out, parent: &Parent) {
    let nest = rd::create(rd::SYSTEM, &rd::spec(600, 4, 50)).expect("a budget for a launcher");
    // The launcher gets a handle to its own budget, so it can create a process there. Its own
    // object is charged *here*, so its notice outlives that budget; its child's is charged in
    // `nest`, so that one does not.
    let run = parent.start(nest, R_LAUNCHER, 0, &[nest]).expect("a launcher child");
    // Wait for the launcher to have made its grandchild.
    let mut usage = rd::usage(nest).expect("budget_usage");
    for _ in 0..1000 {
        if usage.processes_usage >= 2 {
            break;
        }
        test_programs::wait_ms(1);
        usage = rd::usage(nest).expect("budget_usage");
    }
    check!(out, usage.processes_usage == 2, "the launcher's budget holds the launcher and its child");
    rd::destroy(nest).expect("budget_destroy");
    let first = parent.notice(&run);
    show(out, "no-notice", &first);
    check!(out, is(&first, Cause::Killed, 0, 0, &[]), "the launcher itself is reported killed");
    // The grandchild's object was charged to the destroyed budget, so nothing is owed for it;
    // a second receive on the same endpoint finds nothing at all.
    let second = rd::receive(Some(run.exit), 20_000, 0);
    check!(
        out,
        second == Err(Error::Timeout),
        "a process whose object died with its creator's budget produces no notice"
    );
}

/// `thread_create` and `thread_exit` in this process itself, where the results can be seen
/// without a notice.
fn threads(out: &mut Out) {
    let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).expect("a stack");
    let top = stack + 4 * rd::PAGE_SIZE - 16;
    let first = rd::thread_create(entry_of(exiting_thread), top, 0).expect("thread_create");
    check!(out, first != 0, "thread_create returns a thread id");
    // Every thread slot but this one and the trap thread's, then one more.
    let mut made = 1;
    loop {
        let stack = match rd::map_anon(rd::PAGE_SIZE, rd::rw()) {
            Ok(at) => at,
            Err(_) => break,
        };
        match rd::thread_create(entry_of(parking_thread), stack + rd::PAGE_SIZE - 16, 0) {
            Ok(_) => made += 1,
            Err(e) => {
                check!(
                    out,
                    e == Error::TooManyThreads,
                    "thread {} past the limit -> TooManyThreads",
                    made + 1
                );
                break;
            }
        }
    }
    check!(out, made >= 8, "made {} threads before the limit", made);
}

extern "C" fn exiting_thread(_arg: usize) -> ! {
    rd::thread_exit().ok();
    test_programs::park()
}

extern "C" fn parking_thread(_arg: usize) -> ! { test_programs::park() }

/// The console, once this process has mapped it, so that a panic says so instead of vanishing.
/// A child never maps it, so a child's panic is silent -- but it sleeps rather than spins, which
/// keeps the kernel reaching its idle branch, where the interim timeout sweep lives.
static CONSOLE: AtomicUsize = AtomicUsize::new(0);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: `CONSOLE` holds the console's register page, mapped for this process by the
        // kernel and never unmapped; this is the only use of it after the main thread stopped.
        let mut out = Out(unsafe { MmioSerialPort::new(uart) });
        say!(out, "\n[proc] FAIL: panicked: {}", info);
    }
    sleep()
}
