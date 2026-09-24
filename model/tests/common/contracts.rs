use redoubt_model::{
    invariants::Checker,
    kernel::{Boot, Kernel, Note, Step},
    mutation::Mutation,
    spec::*,
    syscall::*,
    trace,
};

pub struct World {
    pub k: Kernel,
    pub ops: Vec<Op>,
    checker: Checker,
}
impl World {
    pub fn new(mutation: Option<Mutation>) -> Self {
        let k = Kernel::boot(&Boot::default(), mutation).unwrap();
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
        match self.sys(tid, call)?.outcome {
            Outcome::Done(Ok(r)) => Ok(r),
            x => Err(format!("expected value, got {x:?}")),
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

/// Independent examples from the completion table, not kernel-derived expectations.
pub fn ipc_contracts(mutation: Option<Mutation>) -> Result<(), String> {
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

/// Focused R12/R7/I13 contracts for the WP-K5 rules that live in the kernel model rather than the
/// scheduler: timeouts wake without preempting, the equal-instant expiry order, and the
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
            call: Syscall::Receive { h: None, timeout: 5_000, max_transfer: 0 },
        })?;
        w.sys(1, Syscall::Receive { h: None, timeout: FOREVER, max_transfer: 0 })?;
        w.op(Op::Tick { dt: 1_000 })?;
        expect(w.k.sched.current.is_some_and(|c| c.thread == (pa, ta)), "the spinner runs")?;
        let s = w.op(Op::Tick { dt: 4_500 })?;
        expect(
            s.wakes.iter().any(|x| x.tid == tb && x.result == Err(Error::Timeout)),
            "the sleeper timed out",
        )?;
        expect(w.k.now == now + 5_500, "time")?;
        expect(
            w.k.sched.current.is_some_and(|c| c.thread == (pa, ta)),
            "a timeout wake preempted the running thread",
        )?;
        // At its slice end the woken sleeper, ranked ahead of the spinner (wake-first), runs.
        w.op(Op::Tick { dt: 4_500 })?;
        w.op(Op::Tick { dt: 1 })?;
        expect(w.k.sched.current.is_some_and(|c| c.thread == (pb, tb)), "the woken sleeper runs next")?;
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
