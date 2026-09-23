use redoubt_model::{
    invariants::Checker,
    kernel::{Boot, Kernel, Step},
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
