//! Redoubt system calls (KERNEL-SPEC.md) through `redoubt-sys`, for the budget and handle-table
//! cases. Records live on the caller's stack, 8-byte aligned by their `[u64; N]` type.

pub use redoubt_sys::{BudgetSpec, Call, Class, Error, FOREVER, Handle, Labels, Number, Return, Usage};
use redoubt_sys::USAGE_SLOTS;

/// Handles the kernel gives the loader's first program (kernel budget.rs, `boot_budgets`).
pub const ROOT: u32 = 1;
pub const SYSTEM: u32 = 2;
pub const USERS: u32 = 3;

pub fn h(index: u32) -> Handle { Handle::new(index).expect("handle 0") }

pub fn spec(pages: u64, processes: u32, weight: u32) -> BudgetSpec {
    BudgetSpec { pages, processes, weight, class: Class::User, labels: Labels::new(), account: 0, deadline: FOREVER }
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

pub fn random(bytes: usize, len: usize) -> Result<(), Error> {
    redoubt_sys::syscall(&Call::Random { bytes, len }).map(|_| ())
}

/// A call from raw registers `a0..=a7`, as a hostile program makes it; returns `a0` (0 for
/// success, else the error code).
pub fn raw(regs: [usize; 8]) -> usize {
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
    r[0]
}

/// A call's number as it travels in `a0`.
pub fn number(call: Number) -> usize { call as usize }

/// The error a raw call's `a0` names (`None` for success or an unknown code).
pub fn raw_error(a0: usize) -> Option<Error> { Error::from_code(a0 as u64) }

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
        let sid = xous::SID::from_bytes(ADDRESS).unwrap();
        let cid = xous::connect(sid).expect("couldn't connect to the victim");
        xous::send_message(cid, xous::Message::new_blocking_scalar(GO, 0, 0, 0, 0)).expect("victim");
    }
}
