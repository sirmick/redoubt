//! Redoubt system calls (KERNEL-SPEC.md) through `redoubt-sys`, for the budget and handle-table
//! cases. Records live on the caller's stack, 8-byte aligned by their `[u64; N]` type.

use core::num::{NonZeroU64, NonZeroUsize};

pub use redoubt_sys::{
    Body, BudgetSpec, Call, Cause, Error, ExitNotice, FOREVER, Handle, Handles, Labels, MAX_HANDLES,
    MAX_LEND_PAGES, MAX_MSG_HANDLES, MAX_OPEN_CALLS, MAX_START_HANDLES, MemFlags, Message, MessageKind,
    MintSource, Number, PAGE_SIZE, Pages, Received, ReceivedBody, ResetKind, Return, Usage, WAIT_CAP, WORDS,
};
use redoubt_sys::{RECEIVED_SLOTS, USAGE_SLOTS};

/// Handles the kernel gives the loader's first program (kernel budget.rs, `boot_budgets`).
pub const ROOT: u32 = 1;
pub const SYSTEM: u32 = 2;
pub const USERS: u32 = 3;

/// Then every device object the loader made from the device tree, in the loader's order
/// (kernel `device.rs`, `boot_devices`; BOOT.md, `Devs`). **INTERIM** until `init` reads the
/// boot manifest (WP-R3): the Reset right first, then the console `/chosen/stdout-path` names
/// and its interrupt, so a program can name those three without a manifest. Everything after
/// them depends on the machine, so nothing may assume how many there are: use
/// [`first_free`] to find where a program's own handles start.
pub const RESET: u32 = 4;
pub const CONSOLE_MMIO: u32 = 5;
pub const CONSOLE_IRQ: u32 = 6;
/// The first device handle beyond the three the order pins.
pub const OTHER_DEVICES: u32 = 7;

pub fn h(index: u32) -> Handle { Handle::new(index).expect("handle 0") }

/// The lowest index this process's handle table does not hold, found by asking: how many
/// handles a program starts with depends on the machine (a device object per MMIO region and
/// per interrupt), so a program that creates handles of its own works this out instead of
/// counting on a number. `budget_usage` answers `BadHandle` only for an index that holds
/// nothing, and writes nothing then.
///
/// **INTERIM** with the handles above it (WP-K3): once `process_start` passes a handle list
/// (WP-K4) and `init` hands each program its own from the boot manifest (WP-R3), a program
/// is told what it holds and none of this is needed.
pub fn first_free() -> u32 {
    let mut rec = [0u64; USAGE_SLOTS];
    let at = rec.as_mut_ptr() as usize;
    (1..=redoubt_sys::MAX_HANDLES as u32 + 1)
        .find(|i| usage_raw(*i, at) == Err(Error::BadHandle))
        .expect("a table with a free index")
}

// --- Devices and memory (WP-K3) ----------------------------------------------------------

/// `map_device(h(MMIO)) -> addr, len` (QUESTIONS.md 146, pending).
pub fn map_device(device: u32) -> Result<(usize, usize), Error> {
    match redoubt_sys::syscall(&Call::MapDevice { device: h(device) })? {
        Return::Mapping { addr, len } => Ok((addr, len)),
        _ => Err(Error::InvalidArgument),
    }
}

/// `dma_alloc(h(MMIO), npages) -> addr, phys`.
pub fn dma_alloc(device: u32, npages: usize) -> Result<(usize, u64), Error> {
    match redoubt_sys::syscall(&Call::DmaAlloc { device: h(device), npages })? {
        Return::Dma { addr, phys } => Ok((addr, phys)),
        _ => Err(Error::InvalidArgument),
    }
}

/// `system_reset(h(Reset), kind)`. Returns only if the kernel refused.
pub fn system_reset(device: u32, kind: ResetKind) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::SystemReset { device: h(device), kind }).map(|_| ())
}

/// `map_anon(len, flags) -> addr`.
pub fn map_anon(len: usize, flags: MemFlags) -> Result<usize, Error> {
    match redoubt_sys::syscall(&Call::MapAnon { len, flags })? {
        Return::Addr(at) => Ok(at),
        _ => Err(Error::InvalidArgument),
    }
}

pub fn unmap(addr: usize, len: usize) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::Unmap { addr, len }).map(|_| ())
}

pub fn set_flags(addr: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::SetFlags { addr, len, flags }).map(|_| ())
}

/// Read-write pages: what almost every caller wants.
pub fn rw() -> MemFlags { MemFlags::READ | MemFlags::WRITE }

pub fn spec(pages: u64, processes: u32, weight: u32) -> BudgetSpec {
    BudgetSpec { pages, processes, weight, labels: Labels::new(), account: 0, deadline: FOREVER }
}

/// `budget_create`; the new handle's index.
pub fn create(parent: u32, spec: &BudgetSpec) -> Result<u32, Error> {
    let rec = spec.encode();
    create_raw(parent, rec.as_ptr() as usize)
}

/// `budget_create` with the record at any address (hostile cases).
pub fn create_raw(parent: u32, spec_rec: usize) -> Result<u32, Error> {
    match redoubt_sys::syscall(&Call::BudgetCreate { parent: h(parent), spec_rec })? {
        Return::Handle(handle) => Ok(handle.index()),
        _ => Err(Error::InvalidArgument),
    }
}

pub fn destroy(budget: u32) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::BudgetDestroy { budget: h(budget) }).map(|_| ())
}

pub fn close(handle: u32) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::HandleClose { handle: h(handle) }).map(|_| ())
}

pub fn usage(budget: u32) -> Result<Usage, Error> {
    let mut rec = [0u64; USAGE_SLOTS];
    usage_raw(budget, rec.as_mut_ptr() as usize)?;
    Usage::decode(&rec)
}

pub fn usage_raw(budget: u32, usage_rec: usize) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::BudgetUsage { budget: h(budget), usage_rec }).map(|_| ())
}

/// Free pages: limit minus usage.
pub fn free(budget: u32) -> u64 {
    let u = usage(budget).expect("budget_usage");
    u.pages_limit - u.pages_usage
}

pub fn time_now() -> Result<u64, Error> {
    match redoubt_sys::syscall(&Call::TimeNow)? {
        Return::Time(t) => Ok(t),
        _ => Err(Error::InvalidArgument),
    }
}

pub fn random() -> Result<u64, Error> {
    match redoubt_sys::syscall(&Call::Random)? {
        Return::Random(value) => Ok(value),
        _ => Err(Error::InvalidArgument),
    }
}

/// A call from raw registers `a0..=a7`, as a hostile program makes it; returns `a0` (0 for
/// success, else the error code).
pub fn raw(regs: [usize; 8]) -> usize { raw_registers(regs)[0] }

/// All return registers, including ownership facts on IPC errors.
pub fn raw_registers(regs: [usize; 8]) -> [usize; 8] {
    let mut r = regs;
    // SAFETY: `ecall` traps to the kernel, which reads and writes only a0-a7 and memory the call
    // names; callers pass their own addresses, or ones the kernel must refuse.
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") r[0], inlateout("a1") r[1], inlateout("a2") r[2], inlateout("a3") r[3],
            inlateout("a4") r[4], inlateout("a5") r[5], inlateout("a6") r[6], inlateout("a7") r[7],
            options(nostack),
        );
    }
    r
}

/// A call's number as it travels in `a0`.
pub fn number(call: Number) -> usize { call as usize }

/// The error a raw call's `a0` names (`None` for success or an unknown code).
pub fn raw_error(a0: usize) -> Option<Error> { Error::from_code(a0 as u64) }

// --- Endpoints and messages (WP-K2) ----------------------------------------------------------

/// Handle 1 of every bundle program but the first: the boot endpoint (kernel `budget.rs`,
/// `boot_endpoint`, INTERIM). The second program holds it with badge 0, the receive right;
/// every later one with its own PID as the badge.
pub const BOOT_ENDPOINT: u32 = 1;

pub fn endpoint_create() -> Result<u32, Error> {
    match redoubt_sys::syscall(&Call::EndpointCreate)? {
        Return::Handle(handle) => Ok(handle.index()),
        _ => Err(Error::InvalidArgument),
    }
}

/// `mint(source, badge, budget?)`.
pub fn mint(source: MintSource, badge: u64, budget: Option<u32>) -> Result<u32, Error> {
    let badge = NonZeroU64::new(badge).ok_or(Error::InvalidArgument)?;
    let call = Call::Mint { source, badge, budget: budget.map(h) };
    match redoubt_sys::syscall(&call)? {
        Return::Handle(handle) => Ok(handle.index()),
        _ => Err(Error::InvalidArgument),
    }
}

/// `mint` from raw registers, the only way to offer the kernel a badge of 0: `Call::Mint`'s
/// `NonZeroU64` cannot hold one, so a typed call would be refused here rather than there. The
/// registers are the source's tag and its value's two halves, then the badge's two halves, then
/// the optional budget handle (`redoubt-sys`).
pub fn mint_raw(
    tag: usize,
    value: usize,
    badge_low: usize,
    badge_high: usize,
    budget: usize,
) -> Option<Error> {
    raw_error(raw([number(Number::Mint), tag, value, 0, badge_low, badge_high, budget, 0]))
}

pub fn mint_from_handle(endpoint: u32, badge: u64, budget: Option<u32>) -> Result<u32, Error> {
    mint(MintSource::Handle(h(endpoint)), badge, budget)
}

pub fn mint_from_message(msg_id: u64, badge: u64, budget: Option<u32>) -> Result<u32, Error> {
    let id = NonZeroU64::new(msg_id).ok_or(Error::InvalidArgument)?;
    mint(MintSource::Message(id), badge, budget)
}

/// A page range for a lend or a transfer; `None` for neither.
pub fn pages(addr: usize, npages: usize) -> Option<Pages> {
    Some(Pages { addr, npages: NonZeroUsize::new(npages)? })
}

/// Raw IPC outcome; unlike the convenience wrapper this exposes lend consumption on errors.
pub fn call_outcome(
    endpoint: u32,
    body: &Body,
    lend: Option<Pages>,
    timeout: u64,
) -> Result<(redoubt_sys::CallOutcome, Option<ReceivedBody>), Error> {
    let mut rec = body.encode();
    let args = Call::Call { endpoint: h(endpoint), body_rec: rec.as_mut_ptr() as usize, lend, timeout };
    let Return::Call(outcome) = redoubt_sys::syscall(&args)? else { return Err(Error::InvalidArgument) };
    let reply = if outcome.reply_present { Some(ReceivedBody::decode(&rec)?) } else { None };
    Ok((outcome, reply))
}

/// Convenience for tests without ownership assertions. Error replies' handles are closed;
/// tests inspecting a possibly consumed raw lend must use `call_outcome` instead.
pub fn call(endpoint: u32, body: &Body, lend: Option<Pages>, timeout: u64) -> Result<ReceivedBody, Error> {
    let (outcome, reply) = call_outcome(endpoint, body, lend, timeout)?;
    if let Err(error) = outcome.status {
        if let Some(reply) = reply {
            for handle in reply.handles.as_slice().iter().flatten() {
                let _ = redoubt_sys::syscall(&Call::HandleClose { handle: *handle });
            }
        }
        return Err(error);
    }
    reply.ok_or(Error::InvalidArgument)
}

/// `call`, waiting out `Busy`: the endpoint's group is at `WAIT_CAP` (R2). Every program of
/// the bundle lives in `system` with account 0, so they are all one group and share one cap
/// until processes can be created in budgets of their own (WP-K4).
pub fn call_waiting(
    endpoint: u32,
    body: &Body,
    lend: Option<Pages>,
    timeout: u64,
) -> Result<ReceivedBody, Error> {
    loop {
        match call(endpoint, body, lend, timeout) {
            Err(Error::Busy) => crate::wait_ms(1),
            other => return other,
        }
    }
}

/// `send`, waiting out `Busy`, as `call_waiting` does.
pub fn send_waiting(endpoint: u32, body: &Body, transfer: Option<Pages>, timeout: u64) -> Result<(), Error> {
    loop {
        match send(endpoint, body, transfer, timeout) {
            Err(Error::Busy) => crate::wait_ms(1),
            other => return other,
        }
    }
}

/// The first word of a message's buffer.
pub fn peek_pages(pages: Pages) -> u64 { peek(pages.addr) }

/// `send(h, body, transfer, timeout)`.
pub fn send(endpoint: u32, body: &Body, transfer: Option<Pages>, timeout: u64) -> Result<(), Error> {
    let rec = body.encode();
    let args = Call::Send { endpoint: h(endpoint), body_rec: rec.as_ptr() as usize, transfer, timeout };
    redoubt_sys::syscall(&args).map(|_| ())
}

/// `receive(h or none, timeout, max_transfer)`.
pub fn receive(from: Option<u32>, timeout: u64, max_transfer: usize) -> Result<Received, Error> {
    let mut rec = [0u64; RECEIVED_SLOTS];
    let args =
        Call::Receive { from: from.map(h), timeout, max_transfer, received_rec: rec.as_mut_ptr() as usize };
    redoubt_sys::syscall(&args)?;
    Received::decode(&rec)
}

/// `reply(msg_id, body)`.
pub fn reply(msg_id: u64, body: &Body) -> Result<(), Error> {
    let rec = body.encode();
    let id = NonZeroU64::new(msg_id).ok_or(Error::InvalidArgument)?;
    redoubt_sys::syscall(&Call::Reply { msg_id: id, body_rec: rec.as_ptr() as usize }).map(|_| ())
}

/// `serve(msg_id)`.
pub fn serve(msg_id: u64) -> Result<(), Error> {
    let id = NonZeroU64::new(msg_id).ok_or(Error::InvalidArgument)?;
    redoubt_sys::syscall(&Call::Serve { msg_id: id }).map(|_| ())
}

/// A body of four words and no handles.
pub fn body(words: [usize; WORDS]) -> Body { Body { words, handles: Handles::new() } }

/// A body of four words and some handles.
pub fn body_with(words: [usize; WORDS], handles: &[u32]) -> Body {
    let mut list = Handles::new();
    for index in handles {
        list.push(h(*index)).expect("MAX_MSG_HANDLES");
    }
    Body { words, handles: list }
}

/// A page of this process's own memory, for lending and transferring. Touched, so that the
/// kernel is not asked to back it while it decodes (answer 115).
pub fn page() -> usize {
    let range =
        redoubt_abi::map_memory(None, None, 4096, redoubt_abi::MemoryFlags::R | redoubt_abi::MemoryFlags::W)
            .expect("map a page");
    let at = range.as_mut_ptr() as usize;
    // SAFETY: the first word of a page this process just mapped read-write.
    unsafe { (at as *mut u64).write_volatile(0) };
    at
}

/// `npages` contiguous pages of this process's own memory, all touched.
pub fn many_pages(npages: usize) -> usize {
    let flags = redoubt_abi::MemoryFlags::R | redoubt_abi::MemoryFlags::W;
    let range = redoubt_abi::map_memory(None, None, npages * 4096, flags).expect("map pages");
    let at = range.as_mut_ptr() as usize;
    for i in 0..npages {
        // SAFETY: the first word of each page of a range this process just mapped read-write.
        unsafe { ((at + i * 4096) as *mut u64).write_volatile(0) };
    }
    at
}

/// The first word of a page this process can read.
pub fn peek(at: usize) -> u64 {
    // SAFETY: the caller passes a page mapped in this process; a `u64` read of its first word.
    unsafe { (at as *const u64).read_volatile() }
}

/// Write the first word of a page this process can write.
pub fn poke(at: usize, value: u64) {
    // SAFETY: as `peek`, and the caller passes a writable page.
    unsafe { (at as *mut u64).write_volatile(value) };
}

/// Protocol for the budget attack cases: a victim living in `system` beside the attacker waits
/// for the attacker's go, then maps and touches pages in `system` and reports to the checker.
pub mod victim {
    /// Well-known address of the victim's server.
    pub const ADDRESS: &[u8; 16] = b"redoubt-bud-vict";
    /// BlockingScalar: the attacker has made its attempts.
    pub const GO: usize = 1;
    /// Pages the victim maps and touches afterwards.
    pub const PAGES: usize = 64;

    /// Tell the victim the attempts are over. Returns once it has the message.
    pub fn go() {
        let sid = redoubt_abi::SID::from_bytes(ADDRESS).unwrap();
        let cid = redoubt_abi::connect(sid).expect("couldn't connect to the victim");
        redoubt_abi::send_message(cid, redoubt_abi::Message::new_blocking_scalar(GO, 0, 0, 0, 0))
            .expect("victim");
    }
}

// --- Processes and threads (WP-K4) -------------------------------------------------------

/// `process_create(h(budget), h(exit endpoint)) -> h(process)`.
pub fn process_create(budget: u32, exit_endpoint: u32) -> Result<u32, Error> {
    let call = Call::ProcessCreate { budget: h(budget), exit_endpoint: h(exit_endpoint) };
    match redoubt_sys::syscall(&call)? {
        Return::Handle(handle) => Ok(handle.index()),
        _ => Err(Error::InvalidArgument),
    }
}

/// `process_map(h(process), src, dst, len, flags)`.
pub fn process_map(process: u32, src: usize, dst: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::ProcessMap { process: h(process), src, dst, len, flags }).map(|_| ())
}

/// `process_start(h(process), entry, sp, arg, handles)`: the handles land in the child's
/// slots 1..n.
pub fn process_start(
    process: u32,
    entry: usize,
    sp: usize,
    arg: usize,
    handles: &[u32],
) -> Result<(), Error> {
    let mut list = [0u64; MAX_START_HANDLES];
    for (slot, index) in list.iter_mut().zip(handles) {
        *slot = h(*index).to_raw();
    }
    start_raw(process, entry, sp, arg, list.as_ptr() as usize, handles.len() as u32)
}

/// `process_start` with the list at any address and any count (hostile cases).
pub fn start_raw(
    process: u32,
    entry: usize,
    sp: usize,
    arg: usize,
    handles_rec: usize,
    count: u32,
) -> Result<(), Error> {
    let call = Call::ProcessStart { process: h(process), entry, sp, arg, handles_rec, count };
    redoubt_sys::syscall(&call).map(|_| ())
}

/// `thread_create(entry, sp, arg) -> tid`.
pub fn thread_create(entry: usize, sp: usize, arg: usize) -> Result<u32, Error> {
    match redoubt_sys::syscall(&Call::ThreadCreate { entry, sp, arg })? {
        Return::Tid(tid) => Ok(tid),
        _ => Err(Error::InvalidArgument),
    }
}

/// `thread_exit()`. Returns only if the kernel refused.
pub fn thread_exit() -> Result<(), Error> { redoubt_sys::syscall(&Call::ThreadExit).map(|_| ()) }

/// `process_exit(code)`. Never returns on success.
pub fn process_exit(code: u32) -> ! {
    redoubt_sys::syscall(&Call::ProcessExit { code }).ok();
    crate::park()
}
