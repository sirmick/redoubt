use redoubt_model::{
    invariants::Checker,
    kernel::{Boot, DeviceKind, Kernel, Note, Object, Step},
    mutation::Mutation,
    policy::{self, HashRef, PolicyOp, ReqRef, Run},
    spec::*,
    syscall::*,
    trace,
};
use redoubt_steward::{consts::DECLASSIFY_MAX, event::Content};

pub struct World {
    pub k: Kernel,
    pub ops: Vec<Op>,
    checker: Checker,
}
impl World {
    pub fn new(mutation: Option<Mutation>) -> Self { Self::booted(&Boot::default(), mutation) }

    pub fn booted(boot: &Boot, mutation: Option<Mutation>) -> Self {
        let k = Kernel::boot(boot, mutation).unwrap();
        let checker = Checker::new(&k);
        Self { k, ops: Vec::new(), checker }
    }

    pub fn op(&mut self, op: Op) -> Result<Step, String> {
        let s = self.k.step(&op).ok_or_else(|| format!("illegal event: {op:?}"))?;
        self.ops.push(op);
        self.checker.check(&self.k)?;
        Ok(s)
    }

    pub fn sys(&mut self, tid: u64, call: Syscall) -> Result<Step, String> {
        self.op(Op::Sys { pid: 1, tid, call })
    }

    pub fn value(&mut self, tid: u64, call: Syscall) -> Result<Ret, String> {
        self.result(tid, call)?.map_err(|e| format!("expected value, got {e:?}"))
    }

    /// The call's result, success or error; anything but a finished call is a failure.
    pub fn result(&mut self, tid: u64, call: Syscall) -> Result<Result<Ret, Error>, String> {
        match self.sys(tid, call)?.outcome {
            Outcome::Done(r) => Ok(r),
            x => Err(format!("expected a result, got {x:?}")),
        }
    }

    pub fn setup(&mut self) -> Result<(u64, u64, u64), String> {
        let Ret::Handle(ep) = self.value(1, Syscall::EndpointCreate)? else { return Err("endpoint".into()) };
        let Ret::Handle(client) =
            self.value(1, Syscall::Mint { source: MintSource::Handle(ep), badge: 7, budget: None })?
        else {
            return Err("mint".into());
        };
        let Ret::Tid(server) = self.value(1, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })? else {
            return Err("thread".into());
        };
        Ok((ep, client, server))
    }

    pub fn lend(&mut self) -> Result<Buffer, String> {
        let Ret::Addr(addr) = self.value(1, Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R | FLAG_W })?
        else {
            return Err("map".into());
        };
        Ok(Buffer { addr, npages: 1 })
    }

    pub fn call(&mut self, h: u64, lend: Option<Buffer>, timeout: u64) -> Result<Step, String> {
        self.sys(1, Syscall::Call { h, words: [11, 22, 33, 44], handles: vec![], lend, timeout })
    }

    pub fn take(&mut self, ep: u64, server: u64) -> Result<Message, String> {
        match self.value(server, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 })? {
            Ret::Message(m) => Ok(m),
            r => Err(format!("receive: {r:?}")),
        }
    }
}
fn trace_roundtrip(w: &World, mutation: Option<Mutation>) -> Result<(), String> {
    if mutation.is_none() {
        trace::check(&trace::record(&Boot::default(), &w.ops, None)?, None)?;
    }
    Ok(())
}
fn expect(ok: bool, msg: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(msg.into()) } }
pub fn completion(s: &Step) -> Result<&CallCompletion, String> {
    s.wakes
        .iter()
        .find_map(|w| if let Ok(Ret::Call(c)) = &w.result { Some(c) } else { None })
        .ok_or("missing call completion".into())
}

/// A bad record takes nothing (kernel/ipc.md): a receiver whose record went bad while it waited
/// gets `InvalidArgument`, and the abandoned-call notice or interrupt it would have taken arrives
/// on its next `receive` with a good record.
fn bad_record_takes_nothing(mutation: Option<Mutation>) -> Result<(), String> {
    trace_roundtrip(&bad_record_notice(mutation)?, mutation)?;
    trace_roundtrip(&bad_record_interrupt(mutation)?, mutation)
}

/// The traces of [`bad_record_takes_nothing`], for replay.
pub fn bad_record_traces() -> Vec<String> {
    [bad_record_notice(None), bad_record_interrupt(None)]
        .into_iter()
        .map(|w| trace::record(&Boot::default(), &w.unwrap().ops, None).unwrap())
        .collect()
}

fn bad_record_notice(mutation: Option<Mutation>) -> Result<World, String> {
    let mut w = World::new(mutation);
    let (ep, h, server) = w.setup()?;
    w.call(h, None, 1)?;
    let msg = w.take(ep, server)?;
    w.sys(server, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 })?;
    w.op(Op::Record { pid: 1, tid: server, record: Record::Unmapped })?;
    let s = w.op(Op::Tick { dt: 1 })?;
    let told = s.wakes.iter().any(|x| x.tid == server && x.result == Err(Error::InvalidArgument));
    expect(told, "a bad record while an abandoned-call notice arrives: InvalidArgument")?;
    w.op(Op::Record { pid: 1, tid: server, record: Record::Owned })?;
    let r = w.value(server, Syscall::Receive { h: Some(ep), timeout: FOREVER, max_transfer: 0 })?;
    expect(matches!(r, Ret::Abandoned { msg_id } if msg_id == msg.msg_id), "the notice stays owed")?;
    Ok(w)
}

fn bad_record_interrupt(mutation: Option<Mutation>) -> Result<World, String> {
    let mut w = World::new(mutation);
    let (_, _, server) = w.setup()?;
    let irq = w.k.processes[&1]
        .handles
        .iter()
        .find(|(_, x)| {
            matches!(x.object, Object::Device(d) if matches!(w.k.devices[&d].kind, DeviceKind::Irq { n: 10, .. }))
        })
        .map(|(i, _)| *i)
        .ok_or("no IRQ handle")?;
    w.sys(server, Syscall::Receive { h: Some(irq), timeout: FOREVER, max_transfer: 0 })?;
    w.op(Op::Record { pid: 1, tid: server, record: Record::Unmapped })?;
    let s = w.op(Op::Irq { n: 10 })?;
    let told = s.wakes.iter().any(|x| x.tid == server && x.result == Err(Error::InvalidArgument));
    expect(told, "a bad record while an interrupt fires: InvalidArgument")?;
    w.op(Op::Record { pid: 1, tid: server, record: Record::Owned })?;
    let r = w.value(server, Syscall::Receive { h: Some(irq), timeout: FOREVER, max_transfer: 0 })?;
    expect(r == Ret::Interrupt { h: irq }, "the interrupt stays fired")?;
    Ok(w)
}

/// `budget_reap` (kernel/budgets.md, R10): each call destroys one child of the budget, with
/// everything below it, and says how many are left; the budget stays, and once the last child is
/// gone its usage is what it was before the first carve (I10 per child). A child's handle reaps
/// only below that child. Budgets are made from `root` (`init`'s handle 1).
pub fn reap_empties_and_keeps(mutation: Option<Mutation>) -> Result<(), String> {
    let mut w = World::new(mutation);
    let create = |w: &mut World, parent: u64, pages: u64| -> Result<u64, String> {
        let spec = Syscall::BudgetCreate {
            parent,
            pages,
            processes: 0,
            weight: 0,
            labels: vec![],
            account: 0,
            deadline: FOREVER,
        };
        match w.value(1, spec)? {
            Ret::Handle(h) => Ok(h),
            r => Err(format!("budget_create: {r:?}")),
        }
    };
    let usage = |w: &mut World, h: u64| w.result(1, Syscall::BudgetUsage { h });
    let p = create(&mut w, 1, 12)?;
    let empty = usage(&mut w, p)?;
    let a = create(&mut w, p, 1)?;
    let b = create(&mut w, p, 6)?;
    let g = create(&mut w, b, 2)?;
    let gg = create(&mut w, g, 0)?;
    // The first child is the newest, `b`: it goes with its subtree, and `a` stays.
    expect(w.result(1, Syscall::BudgetReap { h: p })? == Ok(Ret::Remaining(1)), "one child left")?;
    for (h, what) in [(b, "the reaped child"), (g, "a grandchild"), (gg, "a great-grandchild")] {
        expect(usage(&mut w, h)? == Err(Error::BadHandle), &format!("{what} is gone"))?;
    }
    expect(usage(&mut w, p)?.is_ok() && usage(&mut w, a)?.is_ok(), "the budget and its other child stay")?;
    // A child's handle reaches only below it: `a` has no children, and its parent keeps it.
    expect(w.result(1, Syscall::BudgetReap { h: a })? == Ok(Ret::Remaining(0)), "a leaf has none")?;
    expect(usage(&mut w, a)?.is_ok(), "a reap keeps the budget it names")?;
    expect(w.result(1, Syscall::BudgetReap { h: p })? == Ok(Ret::Remaining(0)), "the last child")?;
    expect(usage(&mut w, p)? == empty, "emptied, the budget's usage is as before its first carve")?;
    expect(w.result(1, Syscall::BudgetReap { h: p })? == Ok(Ret::Remaining(0)), "an empty budget")?;
    expect(usage(&mut w, p)? == empty, "reaping an empty budget changes nothing")?;
    expect(w.result(1, Syscall::BudgetReap { h: 1 << 20 })? == Err(Error::BadHandle), "no such handle")?;
    expect(w.result(1, Syscall::BudgetReap { h: 0 })? == Err(Error::BadHandle), "handle 0")?;
    let Ret::Handle(ep) = w.value(1, Syscall::EndpointCreate)? else { return Err("endpoint".into()) };
    expect(w.result(1, Syscall::BudgetReap { h: ep })? == Err(Error::WrongObject), "not a budget")?;
    trace_roundtrip(&w, mutation)
}

/// R10: a destruction delivers nothing until its end. Budget B holds K, which reports to init's
/// endpoint E; B's child C holds a receiver S on E, waiting there before init's own receiver R, and
/// reporting to init's endpoint F, where a send stamped with B waits and nobody receives. Destroying
/// B owes K's notice on E, the only one owed there: it goes to R, since S is gone by the time
/// anything is delivered, whatever order the kills take; and the send fails with `Dead`, never
/// delivered. A destruction that delivered as it killed would hand it to S, killed after K (the
/// walk starts at its top, B).
fn destruction_delivers_at_its_end(mutation: Option<Mutation>) -> Result<World, String> {
    let mut w = World::new(mutation);
    let handle = |r: Ret| match r {
        Ret::Handle(h) => Ok(h),
        r => Err(format!("expected a handle, got {r:?}")),
    };
    let tid = |r: Ret| match r {
        Ret::Tid(t) => Ok(t),
        r => Err(format!("expected a thread, got {r:?}")),
    };
    let e = handle(w.value(1, Syscall::EndpointCreate)?)?;
    let f = handle(w.value(1, Syscall::EndpointCreate)?)?;
    // B holds K and carves C, which holds S: more than `budget`'s 64 pages and 2 processes.
    let carve = Syscall::BudgetCreate {
        parent: 3,
        pages: 256,
        processes: 4,
        weight: 20,
        labels: vec![],
        account: 0,
        deadline: FOREVER,
    };
    let b = handle(w.value(1, carve)?)?;
    let c = budget(&mut w, b, 10, FOREVER)?;
    let (k, _) = spawn(&mut w, b, e)?;
    let (s, st) = spawn_with(&mut w, c, f, vec![e])?;
    let stamped =
        handle(w.value(1, Syscall::Mint { source: MintSource::Handle(f), badge: 3, budget: Some(b) })?)?;
    let r = tid(w.value(1, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })?)?;
    let sender = tid(w.value(1, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })?)?;
    w.op(Op::Sys {
        pid: s,
        tid: st,
        call: Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 },
    })?;
    w.sys(r, Syscall::Receive { h: Some(e), timeout: FOREVER, max_transfer: 0 })?;
    let send =
        Syscall::Send { h: stamped, words: [5; WORDS], handles: vec![], transfer: None, timeout: FOREVER };
    expect(w.sys(sender, send)?.outcome == Outcome::Blocked, "the stamped send waits on F")?;
    let step = w.sys(1, Syscall::BudgetDestroy { h: b })?;
    let woke = |t: u64| step.wakes.iter().find(|x| x.tid == t).map(|x| x.result.clone());
    expect(
        matches!(woke(r), Some(Ok(Ret::ExitNotice { pid, cause: Cause::Killed, .. })) if pid == k),
        "the notice owed on E goes to the receiver that survives",
    )?;
    expect(woke(sender) == Some(Err(Error::Dead)), "a send stamped with a dying budget fails with Dead")?;
    expect(!w.k.processes.contains_key(&s), "the receiver in C is gone")?;
    Ok(w)
}

/// The trace of [`destruction_delivers_at_its_end`], for replay.
pub fn destruction_delivery_trace() -> String {
    trace::record(&Boot::default(), &destruction_delivers_at_its_end(None).unwrap().ops, None).unwrap()
}

/// Independent examples from the completion table, not kernel-derived expectations.
pub fn ipc_contracts(mutation: Option<Mutation>) -> Result<(), String> {
    bad_record_takes_nothing(mutation)?;
    trace_roundtrip(&destruction_delivers_at_its_end(mutation)?, mutation)?;
    reap_empties_and_keeps(mutation)?;
    for taken in [false, true] {
        let mut w = World::new(mutation);
        let (ep, h, server) = w.setup()?;
        let lend = w.lend()?;
        w.call(h, Some(lend), 1)?;
        let msg = if taken { Some(w.take(ep, server)?) } else { None };
        let s = w.op(Op::Tick { dt: 1 })?;
        let c = completion(&s)?;
        expect(
            c.status == Err(Error::Timeout)
                && c.reply.is_none()
                && c.lend == if taken { LendDisposition::Consumed } else { LendDisposition::Returned },
            "timeout ownership/record",
        )?;
        if let Some(msg) = msg {
            let r = w.value(server, Syscall::Reply { msg_id: msg.msg_id, words: [0; 4], handles: vec![] })?;
            expect(r == Ret::Replied { delivered: false, installed_mask: 0 }, "abandoned delivery")?;
        }
        trace_roundtrip(&w, mutation)?;
    }
    for record in [
        Record::Owned,
        Record::CopyFault,
        Record::Unmapped,
        Record::ReadOnly,
        Record::Borrowed,
        Record::Device,
    ] {
        let mut w = World::new(mutation);
        let (ep, h, server) = w.setup()?;
        let lend = w.lend()?;
        let before = w.k.processes[&1].handles.clone();
        w.call(h, Some(lend), FOREVER)?;
        let msg = w.take(ep, server)?;
        w.op(Op::Record { pid: 1, tid: 1, record })?;
        let s = w.sys(server, Syscall::Reply { msg_id: msg.msg_id, words: [8; 4], handles: vec![ep, h] })?;
        let c = completion(&s)?;
        let valid = record == Record::Owned;
        expect(c.status == if valid { Ok(()) } else { Err(Error::InvalidArgument) }, "late output status")?;
        expect(
            c.lend == LendDisposition::Returned && c.reply.is_some() == valid,
            "late output ownership/record",
        )?;
        expect(
            s.outcome
                == Outcome::Done(Ok(Ret::Replied {
                    delivered: valid,
                    installed_mask: if valid { 3 } else { 0 },
                })),
            "server disposition/mask",
        )?;
        if !valid {
            expect(w.k.processes[&1].handles == before, "reply rollback changed caller handles")?;
        }
        trace_roundtrip(&w, mutation)?;
    }
    // One free handle slot: first slot commits, second is explicitly missing. On late output
    // failure even that installation rolls back and InvalidArgument outranks OutOfMemory.
    for fail_output in [false, true] {
        let mut w = World::new(mutation);
        let (ep, h, server) = w.setup()?;
        while w.k.processes[&1].handles.len() < (MAX_HANDLES - 1) as usize {
            w.k.mint(1, 1, MintSource::Handle(ep), 7, None).map_err(|e| format!("fill: {e:?}"))?;
            w.ops.push(Op::Sys {
                pid: 1,
                tid: 1,
                call: Syscall::Mint { source: MintSource::Handle(ep), badge: 7, budget: None },
            });
        }
        w.call(h, None, FOREVER)?;
        let msg = w.take(ep, server)?;
        if fail_output {
            w.op(Op::Record { pid: 1, tid: 1, record: Record::CopyFault })?;
        }
        let s = w.sys(server, Syscall::Reply { msg_id: msg.msg_id, words: [9; 4], handles: vec![ep, h] })?;
        let c = completion(&s)?;
        expect(
            c.status == Err(if fail_output { Error::InvalidArgument } else { Error::OutOfMemory }),
            "partial status",
        )?;
        expect(c.lend == LendDisposition::None, "no lend disposition")?;
        if fail_output {
            expect(
                c.reply.is_none() && w.k.processes[&1].handles.len() == (MAX_HANDLES - 1) as usize,
                "partial rollback",
            )?;
        } else {
            let r = c.reply.as_ref().ok_or("partial reply lost")?;
            expect(
                r.words == [9; 4] && r.handles.len() == 2 && r.handles[0] != 0 && r.handles[1] == 0,
                "partial record",
            )?;
        }
        expect(
            s.outcome
                == Outcome::Done(Ok(Ret::Replied {
                    delivered: !fail_output,
                    installed_mask: if fail_output { 0 } else { 1 },
                })),
            "partial delivery mask",
        )?;
        trace_roundtrip(&w, mutation)?;
    }
    Ok(())
}

pub fn partial_reply_trace() -> String {
    let mut w = World::new(None);
    let (ep, h, server) = w.setup().unwrap();
    while w.k.processes[&1].handles.len() < (MAX_HANDLES - 1) as usize {
        w.k.mint(1, 1, MintSource::Handle(ep), 7, None).unwrap();
        w.ops.push(Op::Sys {
            pid: 1,
            tid: 1,
            call: Syscall::Mint { source: MintSource::Handle(ep), badge: 7, budget: None },
        });
    }
    w.call(h, None, FOREVER).unwrap();
    let m = w.take(ep, server).unwrap();
    w.sys(server, Syscall::Reply { msg_id: m.msg_id, words: [5; 4], handles: vec![ep, h] }).unwrap();
    trace::record(&Boot::default(), &w.ops, None).unwrap()
}

/// Quarantine (kernel/devices.md): two children hold DMA memory, the first from the default
/// boot's deaf device (init's handle 9, whose first reset fails), the second from the healthy
/// device (handle 5) while also mapping the deaf one. The first exits and quarantines the deaf
/// device, which sweeps every handle to it: init's `dma_alloc` and `map_device` on its old index
/// then fail. The second exits too, and its budget still carries its quarantined page, because a
/// quarantined device never counts as reset. Random traces rarely name a quarantined device again,
/// or build this co-holder.
pub fn dma_quarantine_trace() -> String {
    let mut w = World::new(None);
    let child = |w: &mut World, handles: Vec<u64>| {
        let Ok(Ret::Handle(ep)) = w.value(1, Syscall::EndpointCreate) else { panic!("endpoint") };
        let call = Syscall::BudgetCreate {
            parent: 3,
            pages: 40,
            processes: 1,
            weight: 10,
            labels: vec![],
            account: 0,
            deadline: FOREVER,
        };
        let Ok(Ret::Handle(b)) = w.value(1, call) else { panic!("budget") };
        let s = w.sys(1, Syscall::ProcessCreate { budget: b, exit_endpoint: ep }).unwrap();
        let (ph, pid) = s
            .notes
            .iter()
            .find_map(|n| if let Note::Process { h, pid, .. } = n { Some((*h, *pid)) } else { None })
            .unwrap();
        let start = Syscall::ProcessStart { process: ph, entry: 0x1000, sp: 0x2000, arg: 0, handles };
        let s = w.sys(1, start).unwrap();
        let tid = s
            .notes
            .iter()
            .find_map(|n| if let Note::Thread { tid, .. } = n { Some(*tid) } else { None })
            .unwrap();
        (b, pid, tid)
    };
    let (_, p1, t1) = child(&mut w, vec![9]);
    let (b2, p2, t2) = child(&mut w, vec![9, 5]);
    w.op(Op::Sys { pid: p1, tid: t1, call: Syscall::DmaAlloc { h: 1, npages: 1 } }).unwrap();
    w.op(Op::Sys { pid: p2, tid: t2, call: Syscall::DmaAlloc { h: 2, npages: 1 } }).unwrap();
    w.op(Op::Sys { pid: p2, tid: t2, call: Syscall::MapDevice { h: 1 } }).unwrap();
    w.op(Op::Sys { pid: p1, tid: t1, call: Syscall::ProcessExit { code: 0 } }).unwrap();
    w.sys(1, Syscall::DmaAlloc { h: 9, npages: 1 }).unwrap();
    w.sys(1, Syscall::MapDevice { h: 9 }).unwrap();
    w.op(Op::Sys { pid: p2, tid: t2, call: Syscall::ProcessExit { code: 0 } }).unwrap();
    w.sys(1, Syscall::BudgetUsage { h: b2 }).unwrap();
    trace::record(&Boot::default(), &w.ops, None).unwrap()
}

pub fn serve_blame_trace() -> String { serve_blame_trace_with_exit(Syscall::ThreadExit, 0) }

pub fn process_exit_blame_trace() -> String {
    serve_blame_trace_with_exit(Syscall::ProcessExit { code: 27 }, 27)
}

fn serve_blame_trace_with_exit(exit: Syscall, code: u64) -> String {
    let mut w = World::new(None);
    let (ep, h, _) = w.setup().unwrap();
    fn spawn(w: &mut World, budget: u64, ep: u64, handles: Vec<u64>) -> (u64, u64) {
        let Ret::Handle(process) = w.value(1, Syscall::ProcessCreate { budget, exit_endpoint: ep }).unwrap()
        else {
            panic!()
        };
        let s = w.sys(1, Syscall::ProcessStart { process, entry: 0, sp: 0, arg: 0, handles }).unwrap();
        s.notes
            .iter()
            .find_map(|n| {
                if let redoubt_model::kernel::Note::Thread { pid, tid } = n {
                    Some((*pid, *tid))
                } else {
                    None
                }
            })
            .unwrap()
    }
    let (server, st) = spawn(&mut w, 2, ep, vec![ep]);
    for account in [11, 22] {
        let Ret::Handle(budget) = w
            .value(
                1,
                Syscall::BudgetCreate {
                    parent: 3,
                    pages: 32,
                    processes: 1,
                    weight: 10,
                    labels: vec![account],
                    account,
                    deadline: FOREVER,
                },
            )
            .unwrap()
        else {
            panic!()
        };
        let (pid, tid) = spawn(&mut w, budget, ep, vec![h]);
        w.op(Op::Sys {
            pid,
            tid,
            call: Syscall::Call { h: 1, words: [account; 4], handles: vec![], lend: None, timeout: FOREVER },
        })
        .unwrap();
        w.op(Op::Sys {
            pid: server,
            tid: st,
            call: Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 },
        })
        .unwrap();
    }
    w.op(Op::Sys { pid: server, tid: st, call: Syscall::Serve { msg_id: 1 } }).unwrap();
    w.op(Op::Sys { pid: server, tid: st, call: exit }).unwrap();
    let r = w.value(1, Syscall::Receive { h: Some(ep), timeout: 0, max_transfer: 0 }).unwrap();
    assert!(matches!(r, Ret::ExitNotice {
        cause: Cause::Faulted, code: got_code, blamed_account: 11, ref blamed_labels, ..
    } if got_code == code && blamed_labels == &[11]));
    trace::record(&Boot::default(), &w.ops, None).unwrap()
}

/// Start a process in the budget `budget` (a handle of init's) reporting to `ep`; its (pid, tid).
fn spawn(w: &mut World, budget: u64, ep: u64) -> Result<(u64, u64), String> {
    spawn_with(w, budget, ep, vec![])
}

/// As `spawn`, handing the child `handles` in slots 1..n.
fn spawn_with(w: &mut World, budget: u64, ep: u64, handles: Vec<u64>) -> Result<(u64, u64), String> {
    let s = w.sys(1, Syscall::ProcessCreate { budget, exit_endpoint: ep })?;
    let (h, pid) = s
        .notes
        .iter()
        .find_map(|n| match n {
            Note::Process { h, pid, .. } => Some((*h, *pid)),
            _ => None,
        })
        .ok_or("process_create gave no process")?;
    let s = w.sys(1, Syscall::ProcessStart { process: h, entry: 0, sp: 0, arg: 0, handles })?;
    let tid = s
        .notes
        .iter()
        .find_map(|n| match n {
            Note::Thread { tid, .. } => Some(*tid),
            _ => None,
        })
        .ok_or("process_start gave no thread")?;
    Ok((pid, tid))
}

fn budget(w: &mut World, parent: u64, weight: u64, deadline: u64) -> Result<u64, String> {
    match w
        .value(
            1,
            Syscall::BudgetCreate {
                parent,
                pages: 64,
                processes: 2,
                weight,
                labels: vec![],
                account: 0,
                deadline,
            },
        )
        .map_err(|e| format!("budget_create: {e}"))?
    {
        Ret::Handle(h) => Ok(h),
        r => Err(format!("budget_create: {r:?}")),
    }
}

/// Focused R12/R7/I13 contracts for the scheduling rules that live in the kernel model rather
/// than the scheduler: timeouts wake without preempting, the equal-instant expiry order, and the
/// free-weight refusals.
pub fn sched_contracts(mutation: Option<Mutation>) -> Result<(), String> {
    // A timeout expiring mid-slice wakes its thread; the running thread keeps the CPU until its
    // slice ends (R12: never preempt on a wake).
    {
        let mut w = World::new(mutation);
        let Ret::Handle(ep) = w.value(1, Syscall::EndpointCreate)? else { return Err("endpoint".into()) };
        let b1 = budget(&mut w, 3, 100, FOREVER)?;
        let b2 = budget(&mut w, 3, 100, FOREVER)?;
        let (pa, ta) = spawn(&mut w, b1, ep)?;
        let (pb, tb) = spawn(&mut w, b2, ep)?;
        let now = w.k.now;
        w.op(Op::Sys {
            pid: pb,
            tid: tb,
            call: Syscall::Receive { h: None, timeout: 500, max_transfer: 0 },
        })?;
        w.sys(1, Syscall::Receive { h: None, timeout: FOREVER, max_transfer: 0 })?;
        w.op(Op::Tick { dt: 100 })?;
        expect(w.k.sched.current.is_some_and(|c| c.thread == (pa, ta)), "the spinner runs")?;
        let s = w.op(Op::Tick { dt: 450 })?;
        expect(
            s.wakes.iter().any(|x| x.tid == tb && x.result == Err(Error::Timeout)),
            "the sleeper timed out",
        )?;
        expect(w.k.now == now + 550, "time")?;
        expect(
            w.k.sched.current.is_some_and(|c| c.thread == (pa, ta)),
            "a timeout wake preempted the running thread",
        )?;
        // At its slice end the woken sleeper, ranked ahead of the spinner (wake-first), runs.
        w.op(Op::Tick { dt: 450 })?;
        w.op(Op::Tick { dt: 1 })?;
        expect(w.k.sched.current.is_some_and(|c| c.thread == (pb, tb)), "the woken sleeper runs next")?;
    }
    // On several harts each hart picks a runnable thread no hart runs: a budget with two threads
    // runs on two harts, never one thread on two, and the third hart takes the other budget.
    {
        let mut s = redoubt_model::sched::Scheduler { mutation, ..Default::default() };
        s.set_harts(3);
        s.add_budget(1, None, 100);
        s.add_budget(2, None, 100);
        for t in [(1, 0), (1, 1)] {
            s.thread_runnable(1, t);
        }
        s.thread_runnable(2, (2, 0));
        let picks: Vec<_> = (0..3).filter_map(|h| s.pick_on(h)).map(|c| c.thread).collect();
        let mut distinct = picks.clone();
        distinct.sort();
        distinct.dedup();
        expect(
            picks.len() == 3 && distinct.len() == 3,
            "no thread runs on two harts, and every hart runs one",
        )?;
        expect(
            s.pick_on(0).is_some() && (0..3).filter_map(|h| s.on(h)).filter(|c| c.budget == 1).count() == 2,
            "the budget with two threads runs on two harts",
        )?;
    }
    // On two harts, a budget whose last thread ends on its hart, ahead of one still running, does
    // not hold the floor at its own pass while the reconcile that takes it out is still to come:
    // it has no thread, so it counts for the floor no more than for the cap set's weight.
    {
        let mut s = redoubt_model::sched::Scheduler { mutation, ..Default::default() };
        s.set_harts(2);
        s.add_budget(1, None, 100);
        s.add_budget(2, None, 100);
        s.thread_runnable(1, (1, 0));
        s.thread_runnable(2, (2, 0));
        expect(s.pick_on(0).is_some() && s.pick_on(1).is_some(), "each budget runs on a hart")?;
        let ahead = if s.on(0).is_some_and(|c| c.budget == 2) { 0 } else { 1 };
        s.run_on(ahead, SLICE);
        s.run_on(1 - ahead, 1);
        s.thread_exited(2, (2, 0));
        let still = s.budgets[&1].pass;
        expect(s.floor <= still, "a budget with no thread left raises the floor to no pass of its own")?;
    }
    // On two harts a budget's own requeue at its slice end is no wait for a hart: a heavy budget
    // whose slice ends before any other floor raise saw it run is capped as it leaves, and the
    // floor follows the light budget's pass, not its own.
    {
        let mut s = redoubt_model::sched::Scheduler { mutation, ..Default::default() };
        s.set_harts(2);
        s.add_budget(1, None, 900);
        s.add_budget(2, None, 100);
        s.thread_runnable(1, (1, 0));
        s.thread_runnable(2, (2, 0));
        expect(s.pick_on(0).is_some_and(|c| c.budget == 1), "the heavy budget runs first")?;
        s.run_on(0, SLICE);
        s.slice_end_on(0);
        expect(s.capped.contains(&1), "the heavy budget is capped as its own slice ends")?;
    }
    // A block is no requeue: a heavy budget whose running thread blocks while its sibling waits
    // for a hart is not capped by it, and the waiting sibling holds the floor at its budget's pass,
    // so the wait is not forfeited to a later lift.
    {
        let mut s = redoubt_model::sched::Scheduler { mutation, ..Default::default() };
        s.set_harts(2);
        s.add_budget(1, None, 100);
        s.add_budget(2, None, 900);
        s.thread_runnable(1, (1, 0));
        s.thread_runnable(2, (2, 0));
        s.thread_runnable(2, (2, 1));
        expect(s.pick_on(0).is_some_and(|c| c.budget == 1), "the light budget runs on hart 0")?;
        expect(s.pick_on(1).is_some_and(|c| c.budget == 2), "the heavy budget runs on hart 1")?;
        s.run_on(1, SLICE / 2);
        let blocked = s.on(1).expect("hart 1 runs").thread;
        s.thread_blocked(2, blocked);
        expect(
            !s.capped.contains(&2),
            "a budget whose thread blocked is not capped while its sibling waits",
        )?;
        s.run_on(0, SLICE);
        s.slice_end_on(0);
        expect(s.floor <= s.budgets[&2].pass, "the waiting sibling's budget holds the floor")?;
    }
    // A slice is user time: the kernel's work between a pick and the thread's return to user
    // mode, here three slices of it, comes out of none of it, so the picked thread still runs a
    // whole slice rather than being preempted at its first instruction (R12).
    {
        let mut s = redoubt_model::sched::Scheduler { mutation, ..Default::default() };
        s.add_budget(1, None, 100);
        s.thread_runnable(1, (1, 0));
        s.reconcile();
        expect(s.pick().is_some(), "the thread is picked")?;
        s.exit_work(3 * SLICE);
        expect(
            s.current.is_some_and(|c| c.slice_left == SLICE),
            "the exit work after a pick came out of its slice",
        )?;
    }
    // At an equal instant, a timeout goes before a budget deadline: a caller whose call its
    // server took gets Timeout (the lend consumed), not Dead from the server's death.
    expiry_order(mutation)?;
    // Free weight: a carve may not leave a process-holding budget (root holds init) with free
    // weight 0, and no process may be created in a budget whose weight is all carved.
    {
        let mut w = World::new(mutation);
        let free = w.k.budgets[&1].weight - w.k.budgets[&1].weight_used;
        let r = w.sys(
            1,
            Syscall::BudgetCreate {
                parent: 1,
                pages: 8,
                processes: 0,
                weight: free,
                labels: vec![],
                account: 0,
                deadline: FOREVER,
            },
        )?;
        expect(r.outcome == Outcome::Done(Err(Error::InvalidArgument)), "carving all of init's free weight")?;
        let Ret::Handle(ep) = w.value(1, Syscall::EndpointCreate)? else { return Err("endpoint".into()) };
        let x = budget(&mut w, 3, 10, FOREVER)?;
        let r = w.sys(
            1,
            Syscall::BudgetCreate {
                parent: x,
                pages: 8,
                processes: 0,
                weight: 10,
                labels: vec![],
                account: 0,
                deadline: FOREVER,
            },
        )?;
        expect(
            matches!(r.outcome, Outcome::Done(Ok(Ret::Handle(_)))),
            "carving all of a process-free budget's weight",
        )?;
        let r = w.sys(1, Syscall::ProcessCreate { budget: x, exit_endpoint: ep })?;
        expect(
            r.outcome == Outcome::Done(Err(Error::InvalidArgument)),
            "a process in a budget with no free weight",
        )?;
    }
    // Owner decision 5: a deschedule charges at least one unit, so a run too short for the clock
    // to see (the kernel's timebase tick) is not free. A thread that blocks the moment it is
    // picked, over and over, still moves its budget's pass.
    {
        use redoubt_model::sched::{MIN_CHARGE, Scheduler};
        let mut s = Scheduler { mutation, ..Scheduler::default() };
        s.add_budget(1, None, 1 << 31);
        let w = 7;
        s.add_budget(2, Some(1), w);
        let before = s.budgets[&2].pass;
        for _ in 0..10 {
            s.thread_runnable(2, (2, 0));
            s.reconcile();
            expect(s.pick().is_some_and(|c| c.budget == 2), "the budget is picked")?;
            s.thread_blocked(2, (2, 0));
            s.reconcile();
        }
        let want = u128::from(10 * MIN_CHARGE * redoubt_model::spec::STRIDE / w);
        expect(
            s.budgets[&2].pass >= before + want,
            "ten zero-length runs charged less than the minimum each (a sub-tick run was free)",
        )?;
    }
    // The pick and switch into a budget are paid by the budget picked, whatever ended the run
    // before: here a block, whose budget pays for the block alone.
    {
        use redoubt_model::sched::Scheduler;
        let mut s = Scheduler { mutation, ..Scheduler::default() };
        s.add_budget(1, None, 1 << 31);
        let w = 10;
        for b in [2, 3] {
            s.add_budget(b, Some(1), w);
            s.thread_runnable(b, (b, 0));
        }
        s.reconcile();
        let a = s.pick().map(|c| c.budget).ok_or("nothing picked")?;
        s.run(100);
        s.thread_blocked(a, (a, 0));
        s.reconcile();
        let b = s.pick().map(|c| c.budget).ok_or("nothing picked after the block")?;
        let (pa, pb) = (s.budgets[&a].pass, s.budgets[&b].pass);
        let work = 1_000;
        s.bill_switch(a, b, work);
        expect(s.budgets[&a].pass == pa, "the budget that blocked paid for the switch into another")?;
        s.run(1);
        s.thread_blocked(b, (b, 0));
        s.reconcile();
        let want = u128::from(work * redoubt_model::spec::STRIDE / w);
        expect(s.budgets[&b].pass >= pb + want, "the budget picked did not pay for the switch into it")?;
    }
    Ok(())
}

/// A caller's timeout and its server budget's deadline fall on one instant; the timeout is
/// processed first (the caller gets Timeout and its lend is consumed). Returns the world, whose
/// ops make a trace that replay checks against the expiry order.
fn expiry_order_world(mutation: Option<Mutation>) -> Result<World, String> {
    let mut w = World::new(mutation);
    let Ret::Handle(ep) = w.value(1, Syscall::EndpointCreate)? else { return Err("endpoint".into()) };
    let Ret::Handle(h) =
        w.value(1, Syscall::Mint { source: MintSource::Handle(ep), badge: 7, budget: None })?
    else {
        return Err("mint".into());
    };
    let t = w.k.now + 3_000;
    let bs = budget(&mut w, 3, 100, t)?;
    // The server runs in the budget whose deadline falls with the caller's timeout.
    let (ps, server) = spawn_with(&mut w, bs, ep, vec![ep])?;
    let lend = w.lend().map_err(|e| format!("lend: {e}"))?;
    let Ret::Tid(caller) =
        w.value(1, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 }).map_err(|e| format!("thread: {e}"))?
    else {
        return Err("thread".into());
    };
    let dt = t - w.k.now;
    w.sys(caller, Syscall::Call { h, words: [0; 4], handles: vec![], lend: Some(lend), timeout: dt })?;
    match w
        .op(Op::Sys {
            pid: ps,
            tid: server,
            call: Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 },
        })?
        .outcome
    {
        Outcome::Done(Ok(Ret::Message(_))) => {}
        x => return Err(format!("server receive: {x:?}")),
    }
    let s = w.op(Op::Tick { dt: dt + 1 })?;
    let c = s.wakes.iter().find(|x| x.tid == caller).ok_or("the caller was not answered")?;
    match &c.result {
        Ok(Ret::Call(cc)) => expect(
            cc.status == Err(Error::Timeout) && cc.lend == LendDisposition::Consumed,
            "equal instant: the timeout must be processed before the budget deadline",
        )?,
        r => return Err(format!("caller: {r:?}")),
    }
    expect(!w.k.budgets.values().any(|b| b.deadline == Some(t)), "the deadline destroyed its budget")?;
    Ok(w)
}

fn expiry_order(mutation: Option<Mutation>) -> Result<(), String> { expiry_order_world(mutation).map(|_| ()) }

/// The trace of [`expiry_order_world`], for replay.
pub fn expiry_order_trace() -> String {
    let w = expiry_order_world(None).expect("the specified model keeps the expiry order");
    trace::record(&Boot::default(), &w.ops, None).unwrap()
}

// -------------------------------------------------------------------------------------------------
// The steward's directed scenarios: each builds in a few ops the state that one break needs, which
// the random families reach only after hundreds or thousands of seeds (kernel/model.md,
// "Mutations"). Sessions are named by the id the steward drew, read back after each login.

/// The directed steward scenario that catches `m`, if it has one, run on `m`. A panic is a
/// failure of I14, as in the families' runner.
pub fn steward_scenario(m: Mutation) -> Option<Result<(), String>> {
    let scenario: fn(Option<Mutation>) -> Result<(), String> = match m {
        Mutation::PolicyDeclassifyUnfit => declassify_unfit,
        Mutation::R2OneCursor => one_cursor,
        Mutation::PolicyAgentOtherSet => agent_other_set,
        _ => return None,
    };
    Some(std::panic::catch_unwind(|| scenario(Some(m))).unwrap_or_else(|p| {
        let what =
            p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
        Err(format!("I14: the model panicked: {}", what.unwrap_or_default()))
    }))
}

/// The session or agent of `principal` with exactly `labels` (the scenarios start one of each).
fn session_of(run: &Run, principal: usize, labels: &[u64]) -> u64 {
    run.st
        .callers()
        .iter()
        .find(|c| c.principal == principal && c.domain.labels().as_slice() == labels)
        .map_or(0, |c| c.id)
}

/// R42 (`PolicyDeclassifyUnfit`): alice's {7} session writes an item that does not fit a
/// declassification (over `DECLASSIFY_MAX`, or not printable), asks to declassify it, and alice
/// approves it on her channel. The steward refuses the request; with the item's check gone, P6
/// sees the item copied out.
pub fn declassify_unfit(mutation: Option<Mutation>) -> Result<(), String> {
    UNFIT.iter().try_for_each(|item| declassify_unfit_item(mutation, item))
}

/// The items that do not fit a declassification: one byte over `DECLASSIFY_MAX`, and one with a
/// control character.
pub const UNFIT: [&[u8]; 2] = [&[b'a'; DECLASSIFY_MAX + 1], b"ring\x07"];

/// [`declassify_unfit`] for one item.
pub fn declassify_unfit_item(mutation: Option<Mutation>, item: &[u8]) -> Result<(), String> {
    let mut run = Run::new(mutation, 0);
    run.apply(&PolicyOp::Login { principal: 0, labels: vec![7], context: String::new(), key: 11 })?;
    let session = session_of(&run, 0, &[7]);
    run.apply(&PolicyOp::WriteItem { session, labels: vec![7], item: 0, bytes: item.to_vec() })?;
    let content = Content::Declassify { labels: vec![7], item: 0 };
    run.apply(&PolicyOp::Submit { session, content, reason: String::from("report") })?;
    run.apply(&PolicyOp::Open { principal: 0, key: 12 })?;
    run.apply(&PolicyOp::Pending { channel: 0 })?;
    let request = ReqRef { session, nth: 0 };
    run.apply(&PolicyOp::Approve { channel: 0, request, hash: HashRef::Own })?;
    Ok(())
}

/// R2 (`R2OneCursor`): alice's and bob's unlabelled sessions and bob's {9} session call the
/// server. The server takes alice's call; then, with the vault's work, bob's {9} call; then bob's
/// unlabelled call and alice's second. One cursor over every group starts the last round after
/// the {9} group with the vault's work and after alice's without, so the unlabelled calls are
/// taken in another order (P10).
pub fn one_cursor(mutation: Option<Mutation>) -> Result<(), String> {
    let mut step = 0;
    let mut next = |run: &Run| {
        step += 1;
        let (alice, bob, vault) =
            (session_of(run, 0, &[]), session_of(run, 1, &[]), session_of(run, 1, &[9]));
        Some(match step {
            1 => PolicyOp::Login { principal: 0, labels: vec![], context: String::new(), key: 11 },
            2 => PolicyOp::Login { principal: 1, labels: vec![], context: String::new(), key: 21 },
            3 => PolicyOp::Login { principal: 1, labels: vec![9], context: String::new(), key: 21 },
            4 => PolicyOp::Work { session: alice },
            5 | 7 | 10 => PolicyOp::Serve,
            6 => PolicyOp::Work { session: vault },
            8 => PolicyOp::Work { session: bob },
            9 => PolicyOp::Work { session: alice },
            _ => return None,
        })
    };
    policy::steward_noninterference_script(mutation, &mut next).map_err(|f| f.message)
}

/// R37 (`PolicyAgentOtherSet`): alice's {7} session asks for an unlabelled agent and alice
/// approves it. The steward refuses the request; with the own-set check gone, it is taken into
/// the unlabelled set, and its record shows in the audit an unlabelled reader reads (P10).
pub fn agent_other_set(mutation: Option<Mutation>) -> Result<(), String> {
    let mut step = 0;
    let mut next = |run: &Run| {
        step += 1;
        let vault = session_of(run, 0, &[7]);
        Some(match step {
            1 => PolicyOp::Login { principal: 0, labels: vec![], context: String::new(), key: 11 },
            2 => PolicyOp::Login { principal: 0, labels: vec![7], context: String::new(), key: 11 },
            3 => PolicyOp::Open { principal: 0, key: 12 },
            4 => PolicyOp::Submit {
                session: vault,
                content: Content::Agent { labels: vec![], lease: 100 * SLICE },
                reason: String::from("index"),
            },
            5 => PolicyOp::Pending { channel: 0 },
            6 => PolicyOp::Approve {
                channel: 0,
                request: ReqRef { session: vault, nth: 0 },
                hash: HashRef::Own,
            },
            7 => PolicyOp::Usage { principal: 0 },
            _ => return None,
        })
    };
    policy::steward_noninterference_script(mutation, &mut next).map_err(|f| f.message)
}
