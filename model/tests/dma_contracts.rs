//! DMA device reset and frame quarantine (WP-K5b, answer 173): the model's own checks, scripted,
//! beside the property families and mutations that search for the same breaks at random. The
//! kernel's boot cases (`tests/dma-rules.toml`, `tests/dma-reset-*.toml`) cover the kernel.

mod common;
use common::contracts::*;
use redoubt_model::{
    kernel::{DeviceKind, Note, Object},
    spec::*,
    syscall::*,
};

/// `init`'s handles to the two DMA devices of the default boot: one whose reset always
/// confirms (device 2), and one whose first reset fails (device 6, the `dma-reset-deaf` feature).
const DMA: u64 = 5;
const DEAF: u64 = 9;
const USERS: u64 = 3;

/// The syscall's own `Result<Ret, Error>`, as process `pid`'s thread `tid` got it.
fn call(w: &mut World, pid: u64, tid: u64, s: Syscall) -> Result<Ret, Error> {
    match w.op(Op::Sys { pid, tid, call: s }).unwrap().outcome {
        Outcome::Done(r) => r,
        x => panic!("expected a value or an error, got {x:?}"),
    }
}

fn handle(r: Result<Ret, Error>) -> u64 {
    let Ok(Ret::Handle(h)) = r else { panic!("expected a handle, got {r:?}") };
    h
}

fn budget(w: &mut World, parent: u64, pages: u64, processes: u64, weight: u64) -> u64 {
    handle(call(
        w,
        1,
        1,
        Syscall::BudgetCreate {
            parent,
            pages,
            processes,
            weight,
            labels: vec![],
            account: 0,
            deadline: FOREVER,
        },
    ))
}

/// A process created in `budget` by `init`, not started: its handle and pid.
fn process(w: &mut World, budget: u64) -> (u64, u64) {
    let ep = handle(call(w, 1, 1, Syscall::EndpointCreate));
    let s =
        w.op(Op::Sys { pid: 1, tid: 1, call: Syscall::ProcessCreate { budget, exit_endpoint: ep } }).unwrap();
    s.notes
        .iter()
        .find_map(|n| if let Note::Process { h, pid, .. } = n { Some((*h, *pid)) } else { None })
        .unwrap()
}

/// A child started in a fresh budget carved from `parent`, holding `handles` in slots 1..n: its
/// budget handle (in `init`), pid and tid.
fn child(w: &mut World, parent: u64, handles: Vec<u64>) -> (u64, u64, u64) {
    let b = budget(w, parent, 40, 1, 10);
    let (ph, pid) = process(w, b);
    let call = Syscall::ProcessStart { process: ph, entry: 0x1000, sp: 0x2000, arg: 0, handles };
    let s = w.op(Op::Sys { pid: 1, tid: 1, call }).unwrap();
    let tid = s
        .notes
        .iter()
        .find_map(|n| if let Note::Thread { tid, .. } = n { Some(*tid) } else { None })
        .unwrap();
    (b, pid, tid)
}

fn dma_alloc(w: &mut World, pid: u64, tid: u64, h: u64, npages: u64) -> u64 {
    let r = call(w, pid, tid, Syscall::DmaAlloc { h, npages });
    let Ok(Ret::AddrPhys { addr, .. }) = r else { panic!("dma_alloc: {r:?}") };
    addr
}

fn used(w: &mut World, b: u64) -> u64 {
    let Ok(Ret::Usage(c)) = call(w, 1, 1, Syscall::BudgetUsage { h: b }) else { panic!("budget_usage") };
    c.pages_usage
}

fn quarantined(w: &World, device: u64) -> bool {
    matches!(w.k.devices[&device].kind, DeviceKind::Mmio { quarantined: true, .. })
}

/// OD2: a DMA page is neither lent, transferred nor moved by `process_map`; `set_flags` works,
/// and `unmap` drops the mapping but keeps the frame, held and charged.
#[test]
fn dma_pages_stay_put() {
    let mut w = World::new(None);
    let (_, client, _) = w.setup().unwrap();
    let addr = dma_alloc(&mut w, 1, 1, DMA, 1);
    let page = Some(Buffer { addr, npages: 1 });

    let s = w.call(client, page, FOREVER).unwrap();
    assert!(
        matches!(
            s.outcome,
            Outcome::Done(Ok(Ret::Call(CallCompletion { status: Err(Error::InvalidArgument), .. })))
        ),
        "lend of a DMA page: {:?}",
        s.outcome
    );
    let send = Syscall::Send { h: client, words: [0; WORDS], handles: vec![], transfer: page, timeout: 0 };
    assert_eq!(call(&mut w, 1, 1, send), Err(Error::InvalidArgument), "transfer of a DMA page");
    let b = budget(&mut w, USERS, 40, 1, 10);
    let (ph, _) = process(&mut w, b);
    let map = Syscall::ProcessMap { process: ph, src: addr, dst: 0x1000_0000, len: PAGE_SIZE, flags: FLAG_R };
    assert_eq!(call(&mut w, 1, 1, map), Err(Error::InvalidArgument), "process_map of a DMA page");

    let flags = Syscall::SetFlags { addr, len: PAGE_SIZE, flags: FLAG_R };
    assert_eq!(call(&mut w, 1, 1, flags), Ok(Ret::Unit));
    assert_eq!(call(&mut w, 1, 1, Syscall::Unmap { addr, len: PAGE_SIZE }), Ok(Ret::Unit));
    let f = *w.k.processes[&1].dma.first().unwrap();
    assert!(
        w.k.frames.get(&f).is_some_and(|fr| fr.dma == Some(2) && fr.payer == 1),
        "unmap keeps the DMA frame, still charged to init's budget"
    );
}

/// A process that allocates DMA memory and exits gives its budget back its pages once every
/// device it could reach has confirmed a reset.
#[test]
fn exit_pools_after_reset() {
    let mut w = World::new(None);
    let (b, pid, tid) = child(&mut w, USERS, vec![DMA]);
    let base = used(&mut w, b);
    let addr = dma_alloc(&mut w, pid, tid, 1, 2);
    assert_eq!(call(&mut w, pid, tid, Syscall::Unmap { addr, len: PAGE_SIZE }), Ok(Ret::Unit));
    assert!(used(&mut w, b) > base);
    w.op(Op::Sys { pid, tid, call: Syscall::ProcessExit { code: 0 } }).unwrap();
    assert!(w.k.frames.values().all(|f| f.dma.is_none()), "both frames pooled, the unmapped one too");
    assert!(!quarantined(&w, 2));
    assert!(used(&mut w, b) < base, "the process's own charges, and its DMA pages, are back");
}

/// OD6 and P1-1: a device whose reset fails quarantines the dying holder's memory and is never
/// handed out again; a co-holder's later death quarantines all of its memory too, the run through
/// the healthy device included, because a quarantined device counts as not reset.
#[test]
fn deaf_device_quarantines_the_co_holder_too() {
    let mut w = World::new(None);
    let (b1, p1, t1) = child(&mut w, USERS, vec![DEAF]);
    let (b2, p2, t2) = child(&mut w, USERS, vec![DEAF, DMA]);
    dma_alloc(&mut w, p1, t1, 1, 1);
    dma_alloc(&mut w, p2, t2, 1, 1);
    dma_alloc(&mut w, p2, t2, 2, 1);
    let held1 = used(&mut w, b1);

    w.op(Op::Sys { pid: p1, tid: t1, call: Syscall::ProcessExit { code: 0 } }).unwrap();
    assert!(quarantined(&w, 6), "the first reset of the deaf device fails");
    assert_eq!(w.k.frames.values().filter(|f| f.quarantined).count(), 1);
    assert!(used(&mut w, b1) >= 1 && used(&mut w, b1) < held1, "the quarantined page stays charged");
    assert_eq!(call(&mut w, 1, 1, Syscall::DmaAlloc { h: DEAF, npages: 1 }), Err(Error::NotPermitted));
    assert_eq!(call(&mut w, 1, 1, Syscall::MapDevice { h: DEAF }), Err(Error::NotPermitted));

    // The deaf device would now reset, but a quarantined slot never counts as reset. The
    // co-holder dies the other way, by a fault.
    w.op(Op::Fault { pid: p2, tid: t2 }).unwrap();
    let q: Vec<_> = w.k.frames.values().filter(|f| f.quarantined).map(|f| f.dma).collect();
    assert_eq!(q.len(), 3, "the co-holder's runs, through both devices, are quarantined: {q:?}");
    assert!(q.contains(&Some(2)));
    assert!(!quarantined(&w, 2), "the healthy device confirmed, and stays in service");
    assert!(used(&mut w, b2) >= 2);
    assert!(matches!(call(&mut w, 1, 1, Syscall::DmaAlloc { h: DMA, npages: 1 }), Ok(Ret::AddrPhys { .. })));
}

/// OD5 and N1: when a budget holding quarantined pages is destroyed, its carve returns to the
/// parent first and the charge then moves there; so a parent already at its limit ends at most
/// at it (I5), and the pages stay charged to a live budget.
#[test]
fn quarantine_charge_moves_to_a_parent_at_its_limit() {
    let mut w = World::new(None);
    let parent = budget(&mut w, USERS, 64, 1, 50);
    let Object::Budget(pid_budget) = w.k.processes[&1].handles[&parent].object else { panic!() };
    let (c, p, t) = child(&mut w, parent, vec![DEAF]);
    dma_alloc(&mut w, p, t, 1, 2);
    w.op(Op::Sys { pid: p, tid: t, call: Syscall::ProcessExit { code: 0 } }).unwrap();
    assert!(quarantined(&w, 6));

    let Ok(Ret::Usage(u)) = call(&mut w, 1, 1, Syscall::BudgetUsage { h: parent }) else { panic!() };
    budget(&mut w, parent, u.pages_limit - u.pages_usage - 1, 0, 0);
    assert_eq!(used(&mut w, parent), u.pages_limit, "the parent is at its limit");

    assert_eq!(call(&mut w, 1, 1, Syscall::BudgetDestroy { h: c }), Ok(Ret::Unit));
    let q: Vec<_> = w.k.frames.values().filter(|f| f.quarantined).map(|f| f.payer).collect();
    assert_eq!(q, vec![pid_budget, pid_budget], "both pages are now the parent's");
    assert!(used(&mut w, parent) <= u.pages_limit, "and it is not over its limit");
}
