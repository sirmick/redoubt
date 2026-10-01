//! The model makes the kernel's checks in the kernel's order (kernel/abi.md, "Errors and the order
//! of checks"): one test per place the two once differed.

mod common;
use common::contracts::*;
use redoubt_model::{
    kernel::{Boot, DEFAULT_BASE, DEFAULT_MESSAGE_BASE, DeviceSpec, Resets},
    mutation::Mutation,
    spec::*,
    syscall::*,
};

/// `init`'s handle to `system` (boot order: `root`, `system`, `users`, then the devices).
const SYSTEM: u64 = 2;

fn spec(processes: u64, labels: usize) -> Syscall {
    Syscall::BudgetCreate {
        parent: SYSTEM,
        pages: 1,
        processes,
        weight: 0,
        labels: (1..=labels as u64).collect(),
        account: 0,
        deadline: FOREVER,
    }
}

#[test]
fn a_budget_spec_decodes_in_slot_order() {
    let mut w = World::new(None);
    // A process count wider than 32 bits (slot 1) comes before too many labels (slot 3).
    assert_eq!(w.result(1, spec(1 << 32, MAX_LABELS + 1)).unwrap(), Err(Error::InvalidArgument));
    assert_eq!(w.result(1, spec(0, MAX_LABELS + 1)).unwrap(), Err(Error::TooLarge));
}

#[test]
fn receive_clears_the_current_call_before_its_record_check() {
    let mut w = World::new(None);
    let (ep, client, server) = w.setup().unwrap();
    w.call(client, None, FOREVER).unwrap();
    w.take(ep, server).unwrap();
    assert!(w.k.threads[&server].current.is_some());
    // A handle wider than 32 bits fails decoding and leaves the current call as it was.
    let wide = Syscall::Receive { h: Some(1 << 32), timeout: 0, max_transfer: 0 };
    assert_eq!(w.result(server, wide).unwrap(), Err(Error::BadHandle));
    assert!(w.k.threads[&server].current.is_some());
    // A record that fails its check still ends the current call.
    w.op(Op::Record { pid: 1, tid: server, record: Record::Unmapped }).unwrap();
    let receive = Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 };
    assert_eq!(w.result(server, receive).unwrap(), Err(Error::InvalidArgument));
    assert_eq!(w.k.threads[&server].current, None);
}

#[test]
fn every_page_of_a_record_is_checked() {
    let mut w = World::new(None);
    let (ep, _, server) = w.setup().unwrap();
    let Ret::Addr(base) =
        w.value(1, Syscall::MapAnon { len: 2 * PAGE_SIZE, flags: FLAG_R | FLAG_W }).unwrap()
    else {
        panic!("map_anon")
    };
    // The receive record's first slot is in the first page, its last in the second.
    let straddling = base + PAGE_SIZE - 8;
    let receive = Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 };
    w.op(Op::Record { pid: 1, tid: server, record: Record::Memory(straddling) }).unwrap();
    assert_eq!(w.result(server, receive.clone()).unwrap(), Err(Error::Timeout));
    w.sys(1, Syscall::SetFlags { addr: base + PAGE_SIZE, len: PAGE_SIZE, flags: FLAG_R }).unwrap();
    assert_eq!(w.result(server, receive.clone()).unwrap(), Err(Error::InvalidArgument));
    // A body (9 slots) at the same address is only read by `send`, so a readable page is enough.
    w.op(Op::Record { pid: 1, tid: 1, record: Record::Memory(straddling) }).unwrap();
    let send = Syscall::Send { h: ep, words: [0; 4], handles: vec![], transfer: None, timeout: 0 };
    assert_eq!(w.result(1, send.clone()).unwrap(), Err(Error::Timeout));
    w.sys(1, Syscall::Unmap { addr: base + PAGE_SIZE, len: PAGE_SIZE }).unwrap();
    assert_eq!(w.result(1, send).unwrap(), Err(Error::InvalidArgument));
}

fn addr(w: &mut World, call: Syscall) -> Result<u64, Error> {
    w.result(1, call).unwrap().map(|r| match r {
        Ret::Addr(a) | Ret::AddrPhys { addr: a, .. } => a,
        r => panic!("expected an address, got {r:?}"),
    })
}

#[test]
fn map_anon_searches_from_the_run_it_placed_last() {
    let mut w = World::new(None);
    let page = Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R | FLAG_W };
    assert_eq!(addr(&mut w, page.clone()), Ok(DEFAULT_BASE));
    assert_eq!(addr(&mut w, page.clone()), Ok(DEFAULT_BASE + PAGE_SIZE));
    w.sys(1, Syscall::Unmap { addr: DEFAULT_BASE, len: PAGE_SIZE }).unwrap();
    // The freed first page is not reused while there is room after the last run.
    assert_eq!(addr(&mut w, page), Ok(DEFAULT_BASE + 2 * PAGE_SIZE));
}

/// A boot with extra devices after the default ones; returns the handle of the first extra.
fn with_devices(extra: &[DeviceSpec]) -> (World, u64) {
    let mut boot = Boot::default();
    let first = 4 + boot.devices.len() as u64;
    boot.devices.extend_from_slice(extra);
    (World::booted(&boot, None), first)
}

fn mmio(base: u64, pages: u64) -> DeviceSpec {
    DeviceSpec::Mmio { base, pages, dma: false, resets: Resets::Always }
}

#[test]
fn map_anons_placement_area_is_256_mib() {
    // Two devices of 128 MiB fill the area; a third finds no room, and a larger one never fits.
    let half = 0x1000_0000 / PAGE_SIZE / 2;
    let (mut w, h) = with_devices(&[mmio(0x2000_0000, half), mmio(0x3000_0000, half), mmio(0x5000_0000, 1)]);
    assert_eq!(addr(&mut w, Syscall::MapDevice { h }), Ok(DEFAULT_BASE));
    assert_eq!(addr(&mut w, Syscall::MapDevice { h: h + 1 }), Ok(DEFAULT_BASE + half * PAGE_SIZE));
    assert_eq!(addr(&mut w, Syscall::MapDevice { h: h + 2 }), Err(Error::OutOfMemory));
    let (mut w, h) = with_devices(&[mmio(0x2000_0000, 2 * half + 1)]);
    assert_eq!(addr(&mut w, Syscall::MapDevice { h }), Err(Error::OutOfMemory));
}

#[test]
fn a_receivers_message_area_is_4_mib() {
    let mut boot = Boot::default();
    boot.root.pages = 4096;
    boot.ram_frames = 4097;
    let mut w = World::booted(&boot, None);
    let (ep, _, server) = w.setup().unwrap();
    // The receiver (process 1 itself) fills all but one page of its message area.
    let area = 0x40_0000 / PAGE_SIZE;
    let fill = Syscall::MapFixed { addr: DEFAULT_MESSAGE_BASE, len: (area - 1) * PAGE_SIZE, flags: FLAG_R };
    assert_eq!(w.result(1, fill).unwrap(), Ok(Ret::Unit));
    let Ok(base) = addr(&mut w, Syscall::MapAnon { len: 2 * PAGE_SIZE, flags: FLAG_R | FLAG_W }) else {
        panic!("map_anon")
    };
    let receive = Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 2 };
    let send = |npages| Syscall::Send {
        h: ep,
        words: [0; 4],
        handles: vec![],
        transfer: Some(Buffer { addr: base, npages }),
        timeout: 0,
    };
    // Two pages find no run in the area: the sender is refused.
    assert!(matches!(w.sys(server, receive.clone()).unwrap().outcome, Outcome::Blocked));
    assert_eq!(w.result(1, send(2)).unwrap(), Err(Error::Refused));
    // One page fits, at the area's last page.
    let s = w.sys(1, send(1)).unwrap();
    assert_eq!(s.outcome, Outcome::Done(Ok(Ret::Unit)));
    let last = DEFAULT_MESSAGE_BASE + (area - 1) * PAGE_SIZE;
    assert!(w.k.processes[&1].space.contains_key(&(last / PAGE_SIZE)));
}

#[test]
fn a_device_has_32_dma_runs() {
    let mut w = World::new(None);
    // `init`'s handles to the two healthy DMA devices of the default boot.
    let (dma, other) = (5, 10);
    let alloc = |h| Syscall::DmaAlloc { h, npages: 1 };
    for _ in 0..MAX_RUNS {
        assert!(addr(&mut w, alloc(dma)).is_ok());
    }
    let used = w.k.budgets[&1].pages_used;
    assert_eq!(addr(&mut w, alloc(dma)), Err(Error::OutOfMemory));
    assert_eq!(w.k.budgets[&1].pages_used, used, "a refused run charges nothing");
    // Unmapping a run does not end it: it lasts until its holder ends.
    w.sys(1, Syscall::Unmap { addr: DEFAULT_BASE, len: PAGE_SIZE }).unwrap();
    assert_eq!(addr(&mut w, alloc(dma)), Err(Error::OutOfMemory));
    // The limit is the device's: another device still has all of its runs.
    assert!(addr(&mut w, alloc(other)).is_ok());
}

#[test]
fn map_fixed_refuses_pages_it_cannot_pay_for_before_an_overlap() {
    // More pages than `init`'s budget holds, over a page it has mapped: the pages alone come
    // first (R22), so the error is `OutOfMemory`, not the overlap's `InvalidArgument`.
    let over = |mutation| {
        let mut w = World::new(mutation);
        assert_eq!(addr(&mut w, Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R }), Ok(DEFAULT_BASE));
        let len = (w.k.budgets[&1].pages_limit + 1) * PAGE_SIZE;
        w.result(1, Syscall::MapFixed { addr: DEFAULT_BASE, len, flags: FLAG_R }).unwrap()
    };
    assert_eq!(over(None), Err(Error::OutOfMemory));
    // With the overlap checked first, the call answers the overlap.
    assert_eq!(over(Some(Mutation::R22MapFixedWalksFirst)), Err(Error::InvalidArgument));
}
