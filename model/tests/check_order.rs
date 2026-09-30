//! The model makes the kernel's checks in the kernel's order (kernel/abi.md, "Errors and the order
//! of checks"): one test per place the two once differed.

mod common;
use common::contracts::*;
use redoubt_model::{spec::*, syscall::*};

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
