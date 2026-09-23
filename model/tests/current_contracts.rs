mod common;
use common::contracts::*;
use redoubt_model::{
    kernel::{Boot, Kernel, Object},
    spec::*,
    syscall::*,
    trace,
};

#[test]
fn ipc_completion_table_and_rollback() { ipc_contracts(None).unwrap(); }
#[test]
fn initial_records_fail_without_delivery() {
    for record in [Record::Unmapped, Record::ReadOnly, Record::Borrowed, Record::Device] {
        let mut w = World::new(None);
        let (_, h, _) = w.setup().unwrap();
        let lend = w.lend().unwrap();
        let pages = w.k.budgets[&1].pages_used;
        w.op(Op::Record { pid: 1, tid: 1, record }).unwrap();
        let s = w.call(h, Some(lend), FOREVER).unwrap();
        assert_eq!(
            s.outcome,
            Outcome::Done(Ok(Ret::Call(CallCompletion {
                status: Err(Error::InvalidArgument),
                lend: LendDisposition::Returned,
                reply: None
            })))
        );
        assert!(w.k.msgs.is_empty());
        assert_eq!(pages, w.k.budgets[&1].pages_used);
        let bad = w.call(0, Some(Buffer { addr: 0, npages: 1 }), 0).unwrap();
        assert!(matches!(
            bad.outcome,
            Outcome::Done(Ok(Ret::Call(CallCompletion {
                status: Err(Error::BadHandle),
                lend: LendDisposition::Returned,
                ..
            })))
        ));
    }
}
#[test]
fn record_inside_returning_lend_and_server_thread_exit() {
    for exits in [false, true] {
        let mut w = World::new(None);
        let (ep, h, server) = w.setup().unwrap();
        let lend = w.lend().unwrap();
        w.op(Op::Record { pid: 1, tid: 1, record: Record::Memory(lend.addr) }).unwrap();
        w.call(h, Some(lend), FOREVER).unwrap();
        let msg = w.take(ep, server).unwrap();
        let s = w
            .sys(
                server,
                if exits {
                    Syscall::ThreadExit
                } else {
                    Syscall::Reply { msg_id: msg.msg_id, words: [4; 4], handles: vec![] }
                },
            )
            .unwrap();
        let c = completion(&s).unwrap();
        assert_eq!(c.lend, LendDisposition::Returned);
        assert_eq!(c.status, if exits { Err(Error::Dead) } else { Ok(()) });
        assert_eq!(c.reply.is_some(), !exits);
        assert!(w.k.processes.contains_key(&1));
        trace::check(&trace::record(&Boot::default(), &w.ops, None).unwrap(), None).unwrap();
    }
}
#[test]
fn occupied_handle_pages_and_cap() {
    let mut w = World::new(None);
    let (ep, _, _) = w.setup().unwrap();
    while w.k.processes[&1].handles.len() < 129 {
        w.k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    }
    let used = w.k.budgets[&1].pages_used;
    for h in 1..129 {
        w.k.handle_close(1, h).unwrap();
    }
    assert_eq!(w.k.budgets[&1].pages_used, used - 1);
    assert!(w.k.processes[&1].handles.contains_key(&129));
    w.k.handle_close(1, 129).unwrap();
    assert_eq!(w.k.budgets[&1].pages_used, used - 2);
    let ep = w.k.endpoint_create(1).unwrap();
    while w.k.processes[&1].handles.len() < MAX_HANDLES as usize {
        w.k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    }
    assert_eq!(w.k.mint(1, 1, MintSource::Handle(ep), 7, None), Err(Error::TooLarge));
}
#[test]
fn contexts_are_separate_from_creator_object_on_both_widths() {
    for contexts in [1, 2] {
        let mut boot = Boot::default();
        boot.costs.contexts = contexts;
        let mut k = Kernel::boot(&boot, None).unwrap();
        let ep = k.endpoint_create(1).unwrap();
        let before_root = k.budgets[&1].pages_used;
        let before_child = k.budgets[&2].pages_used;
        let h = k.process_create(1, 2, ep).unwrap();
        let Object::Process(pid) = k.processes[&1].handles[&h].object else { panic!() };
        assert_eq!(k.budgets[&1].pages_used - before_root, 1);
        assert_eq!(k.budgets[&2].pages_used - before_child, 1 + contexts);
        k.process_start(1, h, 0, 0, 0, &[]).unwrap();
        let tid = *k.processes[&pid].threads.first().unwrap();
        let s = k.step(&Op::Sys { pid, tid, call: Syscall::ThreadExit }).unwrap();
        assert_eq!(s.outcome, Outcome::Gone);
        assert_eq!(k.budgets[&2].pages_used, before_child);
        assert_eq!(k.budgets[&1].pages_used, before_root + 1);
        k.step(&Op::Sys {
            pid: 1,
            tid: 1,
            call: Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 },
        })
        .unwrap();
        assert_eq!(k.budgets[&1].pages_used, before_root);
    }
}
#[test]
fn too_many_labels_precede_bad_scalar_encoding() {
    let mut k = Kernel::boot(&Boot::default(), None).unwrap();
    assert_eq!(k.budget_create(1, 1, 0, u64::MAX, u64::MAX, &[1; 9], 0, FOREVER), Err(Error::TooLarge));
}
#[test]
fn revocation_before_and_after_receipt_has_distinct_ownership() {
    for taken in [false, true] {
        let mut w = World::new(None);
        let (ep, _, server) = w.setup().unwrap();
        let lend = w.lend().unwrap();
        let Ret::Handle(scope) = w
            .value(
                1,
                Syscall::BudgetCreate {
                    parent: 1,
                    pages: 0,
                    processes: 0,
                    weight: 0,
                    labels: vec![],
                    account: 0,
                    deadline: FOREVER,
                },
            )
            .unwrap()
        else {
            panic!()
        };
        let Ret::Handle(h) = w
            .value(1, Syscall::Mint { source: MintSource::Handle(ep), badge: 2, budget: Some(scope) })
            .unwrap()
        else {
            panic!()
        };
        w.call(h, Some(lend), FOREVER).unwrap();
        let msg = if taken { Some(w.take(ep, server).unwrap()) } else { None };
        let s = w.sys(server, Syscall::BudgetDestroy { h: scope }).unwrap();
        let c = completion(&s).unwrap();
        assert_eq!(c.status, Err(Error::Dead));
        assert_eq!(c.reply, None);
        assert_eq!(c.lend, if taken { LendDisposition::Consumed } else { LendDisposition::Returned });
        if let Some(m) = msg {
            assert_eq!(
                w.value(server, Syscall::Reply { msg_id: m.msg_id, words: [0; 4], handles: vec![] }).unwrap(),
                Ret::Replied { delivered: false, installed_mask: 0 }
            );
        }
        trace::check(&trace::record(&Boot::default(), &w.ops, None).unwrap(), None).unwrap();
    }
}
#[test]
fn native_process_exit_with_live_lend_and_last_thread_blame() {
    for fault in [false, true] {
        let mut w = World::new(None);
        let (ep, h, _) = w.setup().unwrap();
        let lend = w.lend().unwrap();
        let Ret::Handle(ph) = w.value(1, Syscall::ProcessCreate { budget: 2, exit_endpoint: ep }).unwrap()
        else {
            panic!()
        };
        let s = w
            .sys(1, Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: vec![ep] })
            .unwrap();
        let (pid, tid) = s
            .notes
            .iter()
            .find_map(|n| {
                if let redoubt_model::kernel::Note::Thread { pid, tid } = n {
                    Some((*pid, *tid))
                } else {
                    None
                }
            })
            .unwrap();
        w.call(h, Some(lend), FOREVER).unwrap();
        w.op(Op::Sys { pid, tid, call: Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 } })
            .unwrap();
        let s = w
            .op(if fault { Op::Fault { pid, tid } } else { Op::Sys { pid, tid, call: Syscall::ThreadExit } })
            .unwrap();
        let c = completion(&s).unwrap();
        assert_eq!(c.status, Err(Error::Dead));
        assert_eq!(c.lend, LendDisposition::Returned);
        assert!(c.reply.is_none());
        assert!(!w.k.processes.contains_key(&pid));
        let notice = w.value(1, Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 }).unwrap();
        assert!(matches!(notice, Ret::ExitNotice { cause: Cause::Faulted, code: 0, .. }));
        trace::check(&trace::record(&Boot::default(), &w.ops, None).unwrap(), None).unwrap();
    }
}
#[test]
fn pid_reuse_only_after_notice_receipt() {
    let mut k = Kernel::boot(&Boot::default(), None).unwrap();
    let ep = k.endpoint_create(1).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut reused = false;
    for _ in 0..1200 {
        let h = k.process_create(1, 2, ep).unwrap();
        let Object::Process(pid) = k.processes[&1].handles[&h].object else { panic!() };
        reused |= !seen.insert(pid);
        k.process_start(1, h, 0, 0, 0, &[]).unwrap();
        let tid = *k.processes[&pid].threads.first().unwrap();
        k.step(&Op::Sys { pid, tid, call: Syscall::ThreadExit }).unwrap();
        let second = k.process_create(1, 2, ep).unwrap();
        let Object::Process(other) = k.processes[&1].handles[&second].object else { panic!() };
        assert_ne!(pid, other);
        k.process_start(1, second, 0, 0, 0, &[]).unwrap();
        let other_tid = *k.processes[&other].threads.first().unwrap();
        k.step(&Op::Sys { pid: other, tid: other_tid, call: Syscall::ThreadExit }).unwrap();
        for _ in 0..2 {
            k.step(&Op::Sys {
                pid: 1,
                tid: 1,
                call: Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 },
            })
            .unwrap();
        }
    }
    assert!(reused, "deterministic seeds should exercise PID reuse");
}

#[test]
fn unsettled_receive_output_is_rejected_by_trace_oracle() {
    let mut w = World::new(None);
    let (ep, _, server) = w.setup().unwrap();
    w.sys(server, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 }).unwrap();
    let op = Op::Record { pid: 1, tid: server, record: Record::Unmapped };
    assert!(w.k.unsupported_receive_output(&op));
    assert!(w.k.step(&op).is_none());
    let mut ops = w.ops.clone();
    ops.push(op);
    assert!(trace::record(&Boot::default(), &ops, None).unwrap_err().contains("171"));
    let mut w = World::new(None);
    let (ep, _, server) = w.setup().unwrap();
    let memory = w.lend().unwrap();
    w.op(Op::Record { pid: 1, tid: server, record: Record::Memory(memory.addr) }).unwrap();
    w.sys(server, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 }).unwrap();
    let op = Op::Sys { pid: 1, tid: 1, call: Syscall::Unmap { addr: memory.addr, len: PAGE_SIZE } };
    assert!(w.k.unsupported_receive_output(&op));
    let mut ops = w.ops.clone();
    ops.push(op);
    assert!(trace::record(&Boot::default(), &ops, None).unwrap_err().contains("171"));
    let mut w = World::new(None);
    let (ep, _, server) = w.setup().unwrap();
    w.op(Op::Record { pid: 1, tid: server, record: Record::CopyFault }).unwrap();
    let op = Op::Sys {
        pid: 1,
        tid: server,
        call: Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 },
    };
    assert!(w.k.unsupported_receive_output(&op));
}

#[test]
fn explicit_partial_and_current_call_traces() {
    for text in [partial_reply_trace(), serve_blame_trace(), process_exit_blame_trace()] {
        trace::check(&text, None).unwrap();
    }
}

#[test]
fn exited_object_handles_and_queued_copies_live_until_notice_receipt() {
    let mut w = World::new(None);
    let (ep, h, receiver) = w.setup().unwrap();
    let Ret::Handle(exit) = w.value(1, Syscall::EndpointCreate).unwrap() else { panic!() };
    let memory = w.lend().unwrap();
    while w.k.processes[&1].handles.len() < 128 {
        w.k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    }
    let Ret::Handle(ph) = w.value(1, Syscall::ProcessCreate { budget: 2, exit_endpoint: exit }).unwrap()
    else {
        panic!()
    };
    assert_eq!(ph, 129);
    let s =
        w.sys(1, Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: vec![] }).unwrap();
    let (pid, tid) = s
        .notes
        .iter()
        .find_map(|n| {
            if let redoubt_model::kernel::Note::Thread { pid, tid } = n { Some((*pid, *tid)) } else { None }
        })
        .unwrap();
    w.sys(1, Syscall::Send { h, words: [0; 4], handles: vec![ph], transfer: None, timeout: FOREVER })
        .unwrap();
    w.op(Op::Sys { pid, tid, call: Syscall::ThreadExit }).unwrap();
    assert!(w.k.processes[&1].handles.contains_key(&ph));
    let before_delivery = w.k.budgets[&1].pages_used;
    let Ret::Message(m) =
        w.value(receiver, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 }).unwrap()
    else {
        panic!()
    };
    let copy = m.handles[0];
    assert_ne!(copy, 0);
    assert_eq!(w.k.processes[&1].handles[&copy].object, Object::Process(pid));
    assert_eq!(w.k.budgets[&1].pages_used, before_delivery);
    assert_eq!(w.k.process_start(1, ph, 0, 0, 0, &[]), Err(Error::NotPermitted));
    assert_eq!(w.k.process_map(1, copy, memory.addr, 0x1000, PAGE_SIZE, FLAG_R), Err(Error::NotPermitted));
    for flags in [0, FLAG_W] {
        assert_eq!(
            w.k.process_map(1, copy, memory.addr, 0x1000, PAGE_SIZE, flags),
            Err(Error::InvalidArgument),
            "invalid permission combinations precede ended execution"
        );
    }
    w.sys(1, Syscall::Send { h, words: [0; 4], handles: vec![copy], transfer: None, timeout: FOREVER })
        .unwrap();
    let before_receipt = w.k.budgets[&1].pages_used;
    let Ret::ExitNotice { pid: notice_pid, .. } =
        w.value(receiver, Syscall::Receive { h: Some(exit), timeout: 0, max_transfer: 0 }).unwrap()
    else {
        panic!()
    };
    assert_eq!(notice_pid, pid);
    assert_eq!(w.k.budgets[&1].pages_used, before_receipt - 2);
    assert!(!w.k.processes[&1].handles.contains_key(&ph));
    assert!(!w.k.processes[&1].handles.contains_key(&copy));
    let Ret::Message(m) =
        w.value(receiver, Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 }).unwrap()
    else {
        panic!()
    };
    assert_eq!(m.handles, vec![0]);
    assert!(!w.k.ghost.slots.contains_key(&pid));
    assert!(!w.k.ghost.owed.contains_key(&pid));
}

#[test]
fn scheduler_stays_fair_past_the_old_pass_saturation_boundary() {
    use redoubt_model::sched::Scheduler;
    let mut s = Scheduler::default();
    s.add_budget(1, 1);
    s.add_budget(2, 1);
    s.wake(1, 1);
    s.charge(1, u64::MAX / STRIDE + SLICE);
    assert!(s.budgets[&1].pass > u64::MAX as u128);
    s.wake(2, 2);
    assert_eq!(s.budgets[&1].pass, s.budgets[&2].pass);
    let mut runs = [0; 2];
    for _ in 0..1000 {
        let (b, _) = s.pick().unwrap();
        runs[(b - 1) as usize] += 1;
        s.charge(b, SLICE);
    }
    assert_eq!(runs, [500, 500]);
}

#[test]
fn sparse_committed_reply_mask_is_positional() {
    use redoubt_model::{ghost::Flow, invariants};
    let mut k = Kernel::boot(&Boot::default(), None).unwrap();
    let ep = k.endpoint_create(1).unwrap();
    let template = k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    let before = k.processes[&1].handles.clone();
    let supplied = before[&template];
    // Independent committed-record witness: the atomic model allocator cannot fail one
    // homogeneous installation and then succeed on the next. This checks the observable
    // mask contract without inventing an interleaving or passing an invalid source handle.
    let a = k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    let b = k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
    k.ghost.flows.clear();
    k.ghost.flows.push(Flow::ReplyCompletion {
        before,
        after: k.processes[&1].handles.clone(),
        supplied: vec![supplied; 3],
        output_valid: true,
        completion: CallCompletion {
            status: Err(Error::OutOfMemory),
            lend: LendDisposition::None,
            reply: Some(ReplyRecord { words: [7; 4], handles: vec![a, 0, b] }),
        },
        delivered: true,
        mask: 0b101,
    });
    invariants::check(&k).unwrap();
    let Flow::ReplyCompletion { mask, .. } = &mut k.ghost.flows[0] else { panic!() };
    *mask = 0b011; // A count-derived prefix must fail for the same committed witness.
    assert!(invariants::check(&k).unwrap_err().contains("not positional"));
}

#[test]
fn process_map_destination_validation_precedes_started_state() {
    let mut w = World::new(None);
    let Ret::Handle(ep) = w.value(1, Syscall::EndpointCreate).unwrap() else { panic!() };
    let a = w.lend().unwrap();
    let b = w.lend().unwrap();
    let Ret::Handle(process) = w.value(1, Syscall::ProcessCreate { budget: 2, exit_endpoint: ep }).unwrap()
    else {
        panic!()
    };
    w.value(1, Syscall::ProcessMap { process, src: a.addr, dst: 0x1000, len: PAGE_SIZE, flags: FLAG_R })
        .unwrap();
    w.value(1, Syscall::ProcessStart { process, entry: 0, sp: 0, arg: 0, handles: vec![] }).unwrap();
    for (dst, flags, expected) in [
        (0x1000, FLAG_R, Error::InvalidArgument),
        (0x2000, FLAG_R, Error::NotPermitted),
        (0x2000, 0, Error::InvalidArgument),
        (0x2000, FLAG_W, Error::InvalidArgument),
    ] {
        let s = w.sys(1, Syscall::ProcessMap { process, src: b.addr, dst, len: PAGE_SIZE, flags }).unwrap();
        assert_eq!(s.outcome, Outcome::Done(Err(expected)));
    }
    trace::check(&trace::record(&Boot::default(), &w.ops, None).unwrap(), None).unwrap();
}
