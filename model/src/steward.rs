//! The model's embedder of the steward's policy core (servers/steward.md, "Two embedders and a
//! reference"): the crate `redoubt-steward`, which ships, bound to the kernel model.
//!
//! Everything here runs on the kernel model. `init` creates the shared server's endpoint and the
//! steward's own, and starts the steward in the `system` budget with the `users` and `system`
//! budgets and both endpoints; the steward starts a **server** (a system-class process, a `littlefsd`
//! stand-in, holding no budget) receiving on the shared one. Every principal's top budget and its
//! fixed sub-budgets (one per label set) are a `budget_create` at boot.
//!
//! The core decides; this file decides nothing about policy. It turns each call into an event,
//! with the kernel model's clock and fresh words from its entropy source, and carries out each
//! batch the core returns as system calls on the kernel model, in order, stopping at the first
//! failure, then reports it back as `Done`. So a session or an agent is a budget carved from its
//! domain's sub-budget, a revocation scope inside it, fresh connections to the steward's endpoint
//! and the server narrowed to that scope, and a process holding them. The kernel's exit notices
//! become events: the server's carries crash blame (servers/steward.md, "Crash blame"), a
//! session's or an agent's process says it is gone. Sessions' work is real calls to the server,
//! so the kernel's invariants apply to everything the policy does, and non-interference (P10,
//! policy.rs) is judged on kernel results.
//!
//! What stays here is the embedder's half: the transport and admission, the volumes and their
//! own write check (R25), the ideal `keyd` that signs every audit record, and the entropy source.
//!
//! What the model leaves out: SSH itself (a login is "this key for this user name"), the
//! approval terminal (an approval channel is "a connection that authenticated with this key"),
//! and real entropy (the words come from a keyed mixer, one stream per domain, where the server
//! draws them from the kernel's `random`). Audit signatures are opaque ideal-keyd tokens, bound
//! to purpose/domain/length/exact bytes; no cryptographic security is inferred. The volumes are
//! maps the steward's batches read and write directly; the reader and writer budgets stand for
//! the processes that would. Confined push/read-down are policy observations only: the
//! confinement check's named exception (servers/init.md, "The confinement check") is not
//! modelled here.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_steward::audit::Audit;
use redoubt_steward::consts::{PENDING_CAP, RANDOM_WORDS};
use redoubt_steward::domain::{Domain, Labels};
use redoubt_steward::effect::{
    Answer, Bytes, Kind, Object, Output, Parent, Produced, Refusal, Rendered, Server, Step, StepFailed, Token,
};
use redoubt_steward::event::{Content, Event, EventKind};
use redoubt_steward::manifest::{self, Manifest};
use redoubt_steward::{Policy, Store, decide, inspect};

use crate::kernel::{Boot, INIT_PID, Kernel, Limits, Note};
use crate::mutation::Mutation;
use crate::spec::{Counters, Error, FOREVER, WORDS};
use crate::syscall::{Message, MintSource, Op, Outcome, Ret, Syscall};

/// The steward's handle to `users`, the parent of every top budget and crossing budget.
const USERS: u64 = 1;
/// The slot of a session's or an agent's connection to the server: its first shared server
/// (the steward's connection comes first).
const SERVER_SLOT: u64 = 2;
/// The entropy streams that belong to no domain: the events that draw no id, and `sshd`'s
/// channel ids.
const SPARE: u64 = 0x5ba2e;
const SSHD: u64 = 0x55d;

/// Why the embedder, a volume or the kernel said no: the core's own refusals are its `Answer`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    BadManifest,
    UnknownSession,
    /// A signing purpose `keyd`'s grant does not cover.
    BadKey,
    /// A share or bucket is full (serving.rs).
    Cap,
    /// A volume's label check (R25): a read needs the caller's labels to include the item's, a
    /// write needs them equal.
    NotOwner,
    /// A confined labelled caller reads no shared unlabelled volume.
    ReadDown,
    Kernel(Error),
}

type Res<T> = Result<T, Denied>;

/// A process the steward runs or watches: (pid, a tid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u64,
    pub tid: u64,
}

/// What a batch's step made, by its token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Made {
    Budget { id: u64, h: u64 },
    Scope(u64),
    Connection(u64),
    Process(u64),
}

/// The token slot of the process a session's or a lease's batch launches (`redoubt_steward`'s
/// effects).
const PROCESS: u8 = 2;

/// One write to a volume, as the model records it for the families (P6, P11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    /// The session or agent that wrote, or the owner of the batch that did.
    pub by: Object,
    /// The kernel labels of the budget it went through: the writer's own, or a crossing
    /// budget's. `None` for the steward's own write.
    pub through: Option<Vec<u64>>,
    pub labels: Vec<u64>,
    pub item: u64,
    pub bytes: Vec<u8>,
}

/// A session or an agent, as the families name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub id: u64,
    pub kind: Kind,
    pub domain: Domain,
    pub principal: usize,
}

impl Caller {
    pub fn object(&self) -> Object { Object { domain: self.domain.clone(), kind: self.kind, id: self.id } }

    pub fn labelled(&self) -> bool { !self.domain.labels().is_empty() }
}

#[derive(Clone)]
pub struct Steward {
    pub mutation: Option<Mutation>,
    pub k: Kernel,
    pub me: Proc,
    /// The server, and the steward's handles: its receive right to the server's endpoint, its own
    /// endpoint (which sessions' badges route on), and the endpoint it names as every process's
    /// exit endpoint.
    pub server: Proc,
    srv: u64,
    own: u64,
    exits: u64,
    /// The core.
    pub store: Store,
    /// Each principal's top budget, by account, and each domain's fixed sub-budget: (kernel id,
    /// the steward's handle).
    pub tops: BTreeMap<u64, (u64, u64)>,
    pub subs: BTreeMap<Domain, (u64, u64)>,
    made: BTreeMap<Token, Made>,
    /// The session or agent each process runs, by pid.
    pub procs: BTreeMap<u64, Object>,
    /// The volumes: (labels, item) -> bytes.
    pub volumes: BTreeMap<(Vec<u64>, u64), Vec<u8>>,
    pub writes: Vec<Write>,
    /// Every reply, notice and screen the core emitted, in order.
    pub outputs: Vec<Output>,
    pub audit: Vec<Audit>,
    pub audit_signatures: Vec<AuditSignature>,
    audit_keyd: AuditKeyd,
    pub confined: bool,
    /// Every message the server took, in order: its sender's labels and first word (a session's
    /// call carries the session's id). P10 reads the unlabelled ones' order (R2).
    pub taken: Vec<(Vec<u64>, u64)>,
    secret: u64,
    /// Words drawn so far, per entropy stream.
    drawn: BTreeMap<u64, u64>,
    /// `PolicySequentialIds`'s counter.
    counter: u64,
    replies: u64,
    /// When set, every event the core decides, in order: a trace for the Elixir reference
    /// (servers/steward.md, "Two embedders and a reference").
    pub recorded: Option<Vec<Event>>,
    /// Each context's relay, by its token, and the channel console attached to it now (P18).
    pub attached: BTreeMap<Token, u64>,
    /// Channel consoles handed out so far: each attach's is a new one.
    consoles: u64,
    /// A relay given a channel while it held another: two channels attached at once (P18).
    pub both_attached: Option<Object>,
    /// Every note a relay was given to write, by the batch's owner, in order (P18).
    pub notes: Vec<(Object, String)>,
}

/// splitmix64's finaliser: the model's stand-in for a keyed random function.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A domain's entropy stream: an id another domain's events never move.
fn stream(account: u64, labels: &[u64]) -> u64 {
    let set = Labels::new(labels).unwrap_or_default();
    set.as_slice().iter().fold(mix(account ^ 0x5e55), |h, l| mix(h ^ l))
}

fn of(d: &Domain) -> u64 { stream(d.account().get(), d.labels().as_slice()) }

fn limits(l: &manifest::Limits) -> Limits {
    Limits { pages: l.pages, processes: l.processes, weight: l.weight }
}

impl Steward {
    /// Check the manifest and boot the core, boot the kernel model, have `init` start the
    /// steward in `system`; the steward starts the server and carves what boot carves: each
    /// principal's top budget and its fixed sub-budgets.
    pub fn new(manifest: &Manifest, secret: u64, mutation: Option<Mutation>) -> Res<Steward> {
        Steward::with_policy(manifest, secret, mutation, crate::mutation::policy(mutation))
    }

    /// As `new`, with the core deciding by `policy`: the coverage instrument's table, whose entries
    /// record that they ran and then do what the shipped ones do (model/tests/steward_reach.rs).
    pub fn with_policy(
        manifest: &Manifest,
        secret: u64,
        mutation: Option<Mutation>,
        policy: Policy,
    ) -> Res<Steward> {
        let (store, carves) = Store::boot(manifest, policy).ok_or(Denied::BadManifest)?;
        let boot = Boot {
            root: Limits { pages: 4096, processes: 64, weight: 1000 },
            system: Limits { pages: 512, processes: 8, weight: 250 },
            users: Limits { pages: 3000, processes: 48, weight: 500 },
            ram_frames: 4097,
            ..Boot::default()
        };
        let mut k = Kernel::boot(&boot, mutation).map_err(|_| Denied::BadManifest)?;
        let init = Proc { pid: INIT_PID, tid: *k.processes[&INIT_PID].threads.first().unwrap() };
        let e = handle(run(&mut k, init, Syscall::EndpointCreate)?.0)?;
        // init creates the server's endpoint and the steward's (servers/init.md), so their receive
        // rights are stamped `root` and the steward can narrow connections to scopes anywhere
        // (mint only narrows to the default stamp's descendants).
        let srv = handle(run(&mut k, init, Syscall::EndpointCreate)?.0)?;
        let own = handle(run(&mut k, init, Syscall::EndpointCreate)?.0)?;
        // init's slots: 1 root, 2 system, 3 users. The steward gets users (1), system (2), the
        // server's endpoint (3) and its own (4).
        let ph = handle(run(&mut k, init, Syscall::ProcessCreate { budget: 2, exit_endpoint: e })?.0)?;
        let start =
            Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: vec![3, 2, srv, own] };
        let me = started(run(&mut k, init, start)?.1)?;
        let mut st = Steward {
            mutation,
            k,
            me,
            server: me,
            srv: 3,
            own: 4,
            exits: 0,
            store,
            tops: BTreeMap::new(),
            subs: BTreeMap::new(),
            made: BTreeMap::new(),
            procs: BTreeMap::new(),
            volumes: BTreeMap::new(),
            writes: Vec::new(),
            outputs: Vec::new(),
            audit: Vec::new(),
            audit_signatures: Vec::new(),
            audit_keyd: AuditKeyd::new(1),
            confined: false,
            taken: Vec::new(),
            secret,
            drawn: BTreeMap::new(),
            counter: 0,
            replies: 0,
            recorded: None,
            attached: BTreeMap::new(),
            consoles: 0,
            both_attached: None,
            notes: Vec::new(),
        };
        st.exits = handle(st.sys(Syscall::EndpointCreate)?)?;
        st.start_server()?;
        for c in carves {
            let h = st.budget_create(USERS, limits(&c.top), &[], c.account.get(), FOREVER)?;
            st.tops.insert(c.account.get(), (st.budget_id(h), h));
            for (d, l) in c.subs {
                let sh = st.budget_create(h, limits(&l), d.labels().as_slice(), 0, FOREVER)?;
                st.subs.insert(d, (st.budget_id(sh), sh));
            }
        }
        Ok(st)
    }

    fn broken(&self, m: Mutation) -> bool { self.mutation == Some(m) }

    /// A system call by the steward's thread.
    fn sys(&mut self, call: Syscall) -> Res<Ret> { run(&mut self.k, self.me, call).map(|x| x.0) }

    /// Start the server: a process in `system` receiving on the server endpoint (slot 1), named
    /// with the steward's exit endpoint. servers/init.md: a restarted server receives on the same
    /// endpoint. It is given no budget handle: only `init` and the steward hold system budgets.
    fn start_server(&mut self) -> Res<()> {
        let ph = handle(self.sys(Syscall::ProcessCreate { budget: 2, exit_endpoint: self.exits })?)?;
        let mut handles = vec![self.srv];
        if self.broken(Mutation::PolicyServerHoldsSystemBudget) {
            handles.push(2);
        }
        let start = Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles };
        let (_, notes) = run(&mut self.k, self.me, start)?;
        self.server = started(notes)?;
        self.sys(Syscall::HandleClose { h: ph })?;
        Ok(())
    }

    fn budget_create(
        &mut self,
        parent: u64,
        l: Limits,
        labels: &[u64],
        account: u64,
        deadline: u64,
    ) -> Res<u64> {
        let call = Syscall::BudgetCreate {
            parent,
            pages: l.pages,
            processes: l.processes,
            weight: l.weight,
            labels: labels.to_vec(),
            account,
            deadline,
        };
        handle(self.sys(call)?)
    }

    fn budget_id(&self, h: u64) -> u64 {
        match self.k.processes[&self.me.pid].handles.get(&h).map(|x| x.object) {
            Some(crate::kernel::Object::Budget(b)) => b,
            _ => 0,
        }
    }

    /// The kernel's counters for a budget the steward holds, as `budget_usage` returns them.
    pub fn usage(&mut self, h: u64) -> Option<Counters> {
        match self.sys(Syscall::BudgetUsage { h }) {
            Ok(Ret::Usage(c)) => Some(c),
            _ => None,
        }
    }

    pub fn tick(&mut self, dt: u64) {
        self.k.step(&Op::Tick { dt: dt.min(crate::kernel::MAX_TICK) });
        self.poll();
    }

    // -------------------------------------------------------------------------------------------
    // Events in, effects out.

    /// The fresh words for an event: from the stream of the domain it is about, so no other
    /// domain's events move them (R37), or, broken, a counter every event moves (R36).
    fn words(&mut self, kind: &EventKind) -> [u64; RANDOM_WORDS] {
        let stream = self.stream(kind);
        let mut w = [0; RANDOM_WORDS];
        for x in &mut w {
            if self.broken(Mutation::PolicySequentialIds) {
                self.counter += 1;
                *x = self.counter;
            } else {
                let n = self.drawn.entry(stream).or_insert(0);
                *n += 1;
                *x = mix(self.secret ^ mix(stream ^ mix(*n)));
            }
        }
        w
    }

    /// The entropy stream of the domain whose ids an event draws: a login's, the caller's for a
    /// new lease or request, the request's for an approval (its grant or crossing). The events
    /// that draw no id take theirs from a stream of their own.
    fn stream(&self, kind: &EventKind) -> u64 {
        let routes = &inspect::index(&self.store).routes;
        match kind {
            EventKind::Login { principal, labels, .. } => {
                let fixed = inspect::fixed(&self.store);
                let account = fixed.principal(principal).map_or(0, |p| fixed.principals[p].account.get());
                stream(account, labels)
            }
            EventKind::StartAgent { badge, .. } | EventKind::Submit { badge, .. } => {
                routes.get(badge).map_or(SPARE, |r| of(&r.domain))
            }
            EventKind::Approve { request, .. } => inspect::domains(&self.store)
                .find(|(_, s)| s.requests.contains_key(request))
                .map_or(SPARE, |(d, _)| of(d)),
            _ => SPARE,
        }
    }

    /// R26's admission, as the model stands it in: a domain whose pending requests fill its cap is
    /// busy, and its sessions' and agents' calls wait (here, are refused) unless the tables mark
    /// them ahead of admission. `Submit` is the core's own cap's to answer.
    fn admitted(&self, kind: &EventKind) -> bool {
        let badge = match kind {
            EventKind::StartAgent { badge, .. }
            | EventKind::EndLease { badge, .. }
            | EventKind::EndSession { badge } => badge,
            _ => return true,
        };
        if kind.ahead() && !self.broken(Mutation::PolicyEndLeaseAdmitted) {
            return true;
        }
        let Some(route) = inspect::index(&self.store).routes.get(badge) else { return true };
        inspect::domain(&self.store, &route.domain).is_none_or(|s| s.requests.len() < PENDING_CAP)
    }

    /// One call: the event, every batch it starts carried out and reported, and the exit notices
    /// they cause; the answer to the call, if it got one.
    fn call(&mut self, kind: EventKind) -> Option<Answer> {
        self.replies += 1;
        let reply = self.replies;
        if !self.admitted(&kind) {
            let answer = Answer::Refused(Refusal::Cap);
            self.outputs.push(Output::Reply { to: reply, answer: answer.clone() });
            return Some(answer);
        }
        let from = self.outputs.len();
        self.feed(reply, kind);
        self.poll();
        self.outputs[from..].iter().find_map(|o| match o {
            Output::Reply { to, answer } if *to == reply => Some(answer.clone()),
            _ => None,
        })
    }

    /// Decides an event and the `Done` of every batch that follows from it, one at a time.
    fn feed(&mut self, reply: u64, kind: EventKind) {
        let mut queue = VecDeque::from([(reply, kind)]);
        while let Some((reply, kind)) = queue.pop_front() {
            let event = Event { now: self.k.now, random: self.words(&kind), reply, kind };
            if let Some(r) = &mut self.recorded {
                r.push(event.clone());
            }
            let effects = decide(&mut self.store, event);
            if effects.exit {
                return;
            }
            for o in effects.outputs {
                match o {
                    Output::Audit(a) => self.record(a),
                    Output::Forget(object) => self.forget(&object),
                    o => self.outputs.push(o),
                }
            }
            for b in effects.batches {
                let result = self.batch(&b.owner, &b.steps);
                queue.push_back((0, EventKind::Done { object: b.owner, result }));
            }
        }
    }

    /// The steward's exit notices: the server's blames the call it served (servers/init.md: init
    /// passes each exit notice's blame to the steward) and the server restarts; a session's or an
    /// agent's process is gone.
    pub fn poll(&mut self) {
        loop {
            match self.sys(Syscall::Receive { h: Some(self.exits), timeout: 0, max_transfer: 0 }) {
                Ok(Ret::ExitNotice { pid, blamed_account, blamed_labels, .. }) => {
                    if pid == self.server.pid {
                        self.feed(0, EventKind::Blame { account: blamed_account, labels: blamed_labels });
                        let _ = self.start_server();
                    } else if let Some(object) = self.procs.remove(&pid) {
                        self.feed(0, EventKind::Exited { object });
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }

    /// The object is gone: what its steps made goes with it.
    fn forget(&mut self, object: &Object) {
        let mine: Vec<Token> = self.made.keys().filter(|t| t.owner == *object).cloned().collect();
        for t in mine {
            if let Some(Made::Budget { h, .. } | Made::Scope(h) | Made::Connection(h)) = self.made.remove(&t)
            {
                let _ = self.sys(Syscall::HandleClose { h });
            }
        }
        self.procs.retain(|_, o| o != object);
        self.attached.retain(|t, _| t.owner != *object);
    }

    /// Runs a batch on the kernel model, in order, stopping at the first failure.
    fn batch(&mut self, owner: &Object, steps: &[Step]) -> Result<Vec<Produced>, StepFailed> {
        let mut read: BTreeMap<Token, Vec<u8>> = BTreeMap::new();
        let mut done = Vec::new();
        for (i, s) in steps.iter().enumerate() {
            let error = |e: Denied| StepFailed {
                step: i,
                error: if let Denied::Kernel(e) = e { e as u32 + 1 } else { 0 },
            };
            done.push(self.step(owner, s, &mut read).map_err(error)?);
        }
        Ok(done)
    }

    fn budget(&self, t: &Token) -> Res<(u64, u64)> {
        match self.made.get(t) {
            Some(Made::Budget { id, h }) => Ok((*id, *h)),
            _ => Err(Denied::Kernel(Error::BadHandle)),
        }
    }

    /// One step as system calls.
    fn step(&mut self, owner: &Object, step: &Step, read: &mut BTreeMap<Token, Vec<u8>>) -> Res<Produced> {
        let bad = Denied::Kernel(Error::BadHandle);
        match step {
            Step::CreateBudget { token, parent, limits: l, labels, deadline } => {
                let parent = match parent {
                    Parent::Sub(d) => self.subs.get(d).ok_or(bad)?.1,
                    Parent::Budget(t) => self.budget(t)?.1,
                    Parent::Users => USERS,
                };
                let h =
                    self.budget_create(parent, limits(l), labels.as_slice(), 0, deadline.unwrap_or(FOREVER))?;
                let id = self.budget_id(h);
                self.made.insert(token.clone(), Made::Budget { id, h });
                Ok(Produced::Budget(id))
            }
            Step::CreateScope { scope, budget } => {
                let (id, h) = self.budget(budget)?;
                let labels = self.k.budgets.get(&id).map(|b| b.labels.clone()).unwrap_or_default();
                let s =
                    self.budget_create(h, Limits { pages: 0, processes: 0, weight: 0 }, &labels, 0, FOREVER)?;
                self.made.insert(scope.token().clone(), Made::Scope(s));
                Ok(Produced::Scope)
            }
            Step::Connect { token, scope, server, badge } => {
                let Some(Made::Scope(s)) = self.made.get(scope.token()).copied() else { return Err(bad) };
                let source = match server {
                    Server::Steward => self.own,
                    Server::Shared(_) => self.srv,
                };
                let mint =
                    Syscall::Mint { source: MintSource::Handle(source), badge: *badge, budget: Some(s) };
                let h = handle(self.sys(mint)?)?;
                self.made.insert(token.clone(), Made::Connection(h));
                Ok(Produced::Connection)
            }
            Step::Launch { token, budget, connections } => {
                let (_, h) = self.budget(budget)?;
                let mut handles = Vec::new();
                for c in connections {
                    match self.made.remove(c) {
                        Some(Made::Connection(x)) => handles.push(x),
                        _ => return Err(bad),
                    }
                }
                let ph = handle(self.sys(Syscall::ProcessCreate { budget: h, exit_endpoint: self.exits })?)?;
                let start =
                    Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: handles.clone() };
                let pid = run(&mut self.k, self.me, start).and_then(|(_, n)| started(n)).map(|p| p.pid);
                for h in handles.into_iter().chain([ph]) {
                    let _ = self.sys(Syscall::HandleClose { h });
                }
                let pid = pid?;
                self.procs.insert(pid, owner.clone());
                self.made.insert(token.clone(), Made::Process(pid));
                Ok(Produced::Process(pid))
            }
            // The relay is a process of the context's own budget; what it serves is the server's,
            // not the model's: the model keeps which channel each relay holds.
            Step::LaunchRelay { token, budget } => {
                let (_, h) = self.budget(budget)?;
                let ph = handle(self.sys(Syscall::ProcessCreate { budget: h, exit_endpoint: self.exits })?)?;
                let start =
                    Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: Vec::new() };
                let pid = run(&mut self.k, self.me, start).and_then(|(_, n)| started(n)).map(|p| p.pid);
                let _ = self.sys(Syscall::HandleClose { h: ph });
                let pid = pid?;
                self.procs.insert(pid, owner.clone());
                self.made.insert(token.clone(), Made::Process(pid));
                Ok(Produced::Process(pid))
            }
            Step::Attach { relay, note, .. } => {
                if !matches!(self.made.get(relay), Some(Made::Process(_))) {
                    return Err(bad);
                }
                self.consoles += 1;
                if self.attached.insert(relay.clone(), self.consoles).is_some() {
                    self.both_attached.get_or_insert_with(|| owner.clone());
                }
                self.notes.push((owner.clone(), note.clone()));
                Ok(Produced::Done)
            }
            Step::Detach { relay, note, .. } => {
                self.attached.remove(relay);
                self.notes.push((owner.clone(), note.clone()));
                Ok(Produced::Done)
            }
            Step::DestroyBudget { budget } => {
                let (_, h) = self.budget(budget)?;
                self.made.remove(budget);
                let r = self.sys(Syscall::BudgetDestroy { h });
                let _ = self.sys(Syscall::HandleClose { h });
                r.map(|_| Produced::Done)
            }
            Step::Read { token, through, labels, item } => {
                if let Some(t) = through {
                    self.budget(t)?;
                }
                let bytes =
                    self.volumes.get(&(labels.as_slice().to_vec(), *item)).cloned().unwrap_or_default();
                read.insert(token.clone(), bytes.clone());
                Ok(Produced::Bytes(bytes))
            }
            Step::Write { through, labels, item, bytes } => {
                let through = match through {
                    Some(t) => Some(self.k.ghost.labels(self.budget(t)?.0)),
                    None => None,
                };
                let bytes = match bytes {
                    Bytes::Literal(b) => b.clone(),
                    Bytes::Read(t) => read.get(t).cloned().ok_or(bad)?,
                };
                let labels = labels.as_slice().to_vec();
                self.volumes.insert((labels.clone(), *item), bytes.clone());
                self.writes.push(Write { by: owner.clone(), through, labels, item: *item, bytes });
                Ok(Produced::Done)
            }
        }
    }

    // -------------------------------------------------------------------------------------------
    // What sessions, agents, `sshd` and approval channels ask.

    /// Every session and agent, in the core's state.
    pub fn callers(&self) -> Vec<Caller> {
        let mut all = Vec::new();
        for (d, s) in inspect::domains(&self.store) {
            for x in s.sessions.values() {
                all.push(Caller { id: x.id, kind: Kind::Session, domain: d.clone(), principal: x.principal });
            }
            for x in s.leases.values() {
                all.push(Caller { id: x.id, kind: Kind::Lease, domain: d.clone(), principal: x.principal });
            }
        }
        all
    }

    pub fn caller(&self, id: u64) -> Option<Caller> { self.callers().into_iter().find(|c| c.id == id) }

    /// A session's or an agent's badge on the steward's endpoint; 0, which routes nowhere, for
    /// one that does not run.
    pub fn badge(&self, id: u64) -> u64 {
        inspect::index(&self.store).routes.iter().find(|(_, r)| r.id == id).map_or(0, |(b, _)| *b)
    }

    /// The kernel id of the budget a session's or an agent's batch made.
    pub fn budget_of(&self, o: &Object) -> Option<u64> {
        match self.made.get(&Token { owner: o.clone(), slot: 0 }) {
            Some(Made::Budget { id, .. }) => Some(*id),
            _ => None,
        }
    }

    pub fn principal(&self, name: &str) -> Option<usize> { inspect::fixed(&self.store).principal(name) }

    /// `ssh name@box`, with a label set `ssh name+X@box`, and as a named context `ssh name.C@box`
    /// (`context` empty for the default one), from the client address `from`.
    pub fn login(
        &mut self,
        principal: &str,
        labels: &[u64],
        context: &str,
        key: u64,
        from: &str,
    ) -> Option<Answer> {
        let (principal, context, from) = (String::from(principal), String::from(context), String::from(from));
        self.call(EventKind::Login { principal, labels: labels.to_vec(), context, key, from })
    }

    /// `sshd` says the channel of attachment `attachment` closed.
    pub fn channel_closed(&mut self, attachment: u64) -> Option<Answer> {
        self.call(EventKind::ChannelClosed { session: attachment })
    }

    /// `sshd` is gone: every channel with it.
    pub fn sshd_gone(&mut self) { self.call(EventKind::SshdGone); }

    pub fn end_session(&mut self, session: u64) -> Option<Answer> {
        let badge = self.badge(session);
        self.call(EventKind::EndSession { badge })
    }

    pub fn start_agent(&mut self, session: u64, lease: u64) -> Option<Answer> {
        let badge = self.badge(session);
        self.call(EventKind::StartAgent { badge, lease })
    }

    pub fn submit(&mut self, session: u64, content: Content, reason: &str) -> Option<Answer> {
        let badge = self.badge(session);
        self.call(EventKind::Submit { badge, content, reason: String::from(reason) })
    }

    /// Session or agent `by` ends lease `lease`.
    pub fn end_lease(&mut self, by: u64, lease: u64) -> Option<Answer> {
        let badge = self.badge(by);
        self.call(EventKind::EndLease { badge, lease })
    }

    /// `ssh approve@box` with `key`: `sshd`'s new channel id, and the answer.
    pub fn open_channel(&mut self, principal: &str, key: u64) -> (u64, Option<Answer>) {
        let channel = mix(self.secret ^ mix(SSHD ^ mix(self.drawn.get(&SSHD).copied().unwrap_or(0))));
        *self.drawn.entry(SSHD).or_insert(0) += 1;
        (channel, self.call(EventKind::ApprovalOpened { channel, principal: String::from(principal), key }))
    }

    pub fn close_channel(&mut self, channel: u64) { self.call(EventKind::ApprovalClosed { channel }); }

    /// What an approval channel shows now.
    pub fn pending(&mut self, channel: u64) -> Vec<Rendered> {
        let from = self.outputs.len();
        self.call(EventKind::Pending { channel });
        self.outputs[from..]
            .iter()
            .filter_map(|o| match o {
                Output::Screen { channel: c, screen } if *c == channel => Some(screen.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn approve(&mut self, channel: u64, request: u64, hash: [u8; 32]) -> Option<Answer> {
        self.call(EventKind::Approve { channel, request, hash })
    }

    pub fn deny(&mut self, channel: u64, request: u64) -> Option<Answer> {
        self.call(EventKind::Deny { channel, request })
    }

    // -------------------------------------------------------------------------------------------
    // The volumes: their own label check (R25), not the steward's.

    /// A session or an agent writes an item: the volume takes a write only with equal labels (no
    /// write down, no blind write up).
    pub fn write_item(&mut self, session: u64, labels: &[u64], item: u64, bytes: Vec<u8>) -> Res<()> {
        let c = self.caller(session).ok_or(Denied::UnknownSession)?;
        let labels = Labels::new(labels).ok_or(Denied::NotOwner)?;
        let mine = c.domain.labels();
        let allowed =
            if self.broken(Mutation::PolicyWriteUp) { labels.includes(mine) } else { *mine == labels };
        if !allowed {
            return Err(Denied::NotOwner);
        }
        let through = self.budget_of(&c.object()).map(|b| self.k.ghost.labels(b));
        let labels = labels.as_slice().to_vec();
        self.volumes.insert((labels.clone(), item), bytes.clone());
        self.writes.push(Write { by: c.object(), through, labels, item, bytes });
        Ok(())
    }

    /// A read needs the caller's labels to include the item's; a confined deployment adds the
    /// explicit refusal of a shared unlabelled volume to a labelled caller. This models policy,
    /// not unresolved topology 164.
    pub fn read_volume(&self, session: u64, labels: &[u64], item: u64) -> Res<Vec<u8>> {
        let c = self.caller(session).ok_or(Denied::UnknownSession)?;
        let labels = Labels::new(labels).ok_or(Denied::NotOwner)?;
        if !c.domain.labels().includes(&labels) {
            return Err(Denied::NotOwner);
        }
        if self.confined && c.labelled() && labels.is_empty() {
            return Err(Denied::ReadDown);
        }
        Ok(self.volumes.get(&(labels.as_slice().to_vec(), item)).cloned().unwrap_or_default())
    }

    // -------------------------------------------------------------------------------------------
    // Sessions' work on the kernel: calls to the server, and the server's side.

    /// The process of a session or an agent calls the server (no timeout), first starting another
    /// thread so it can call again while this call waits. The result is what the calling thread
    /// got now.
    pub fn work(&mut self, session: u64) -> Res<Outcome> {
        // The object's own process, never its context's relay: the one its batch's `Launch` made.
        let owner = self.procs.values().find(|o| o.id == session).cloned().ok_or(Denied::UnknownSession)?;
        let pid = match self.made.get(&Token { owner, slot: PROCESS }) {
            Some(Made::Process(pid)) => *pid,
            _ => return Err(Denied::UnknownSession),
        };
        let Some(tid) = self.k.runnable().into_iter().find(|(p, _)| *p == pid).map(|x| x.1) else {
            return Ok(Outcome::Blocked);
        };
        let _ = self.k.step(&Op::Sys { pid, tid, call: Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 } });
        let call = Syscall::Call {
            h: SERVER_SLOT,
            words: [session; WORDS],
            handles: Vec::new(),
            lend: None,
            timeout: FOREVER,
        };
        Ok(self.k.step(&Op::Sys { pid, tid, call }).map_or(Outcome::Gone, |s| s.outcome))
    }

    /// The server answers the calls it holds open (from `hold`), then takes what is waiting and
    /// answers all of it, until nothing is left. An abandoned call is answered too: that frees it.
    pub fn serve(&mut self) {
        let s = self.server;
        let held: Vec<u64> = self.k.threads.get(&s.tid).map_or(Vec::new(), |t| {
            t.serving.iter().filter_map(|m| self.k.msgs.get(m)).map(|m| m.rid).collect()
        });
        for m in held {
            self.reply(m);
        }
        for _ in 0..4096 {
            match self.take() {
                Some(Ret::Message(m)) if m.kind == crate::syscall::MsgKind::Call => self.reply(m.msg_id),
                Some(Ret::Message(_)) | Some(Ret::Abandoned { .. }) => {}
                _ => break,
            }
        }
    }

    /// The server takes one waiting call and keeps it open (it is working on it: its current call);
    /// an abandoned-call notice on the way is answered with a reply, which frees the call.
    pub fn hold(&mut self) -> Option<Message> {
        loop {
            match self.take() {
                Some(Ret::Message(m)) => return Some(m),
                Some(Ret::Abandoned { msg_id }) => self.reply(msg_id),
                _ => return None,
            }
        }
    }

    /// One `receive` by the server, without waiting. A message it takes goes in `taken`.
    fn take(&mut self) -> Option<Ret> {
        let s = self.server;
        let recv = Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 };
        let Outcome::Done(Ok(ret)) = self.k.step(&Op::Sys { pid: s.pid, tid: s.tid, call: recv })?.outcome
        else {
            return None;
        };
        if let Ret::Message(m) = &ret {
            self.taken.push((m.labels.clone(), m.words[0]));
        }
        Some(ret)
    }

    /// The server answers one call it took.
    pub fn reply(&mut self, msg_id: u64) {
        let s = self.server;
        let call = Syscall::Reply { msg_id, words: [0; WORDS], handles: Vec::new() };
        self.k.step(&Op::Sys { pid: s.pid, tid: s.tid, call });
    }

    /// The server crashes (the kernel reports the account of the message it was serving).
    pub fn crash_server(&mut self) {
        let s = self.server;
        self.k.step(&Op::Fault { pid: s.pid, tid: s.tid });
        self.poll();
    }

    // -------------------------------------------------------------------------------------------
    // The audit log.

    /// Every record the core emits, signed through the ideal `keyd`.
    fn record(&mut self, record: Audit) {
        let signature = self.audit_keyd.sign("audit", &audit_bytes(&record)).expect("audit-purpose grant");
        self.audit.push(record);
        self.audit_signatures.push(signature);
    }

    /// Per-record authentication only: dropping or reordering valid record/signature pairs is
    /// outside the audit log's guarantee. The operator verifier (servers/steward.md, "Retention,
    /// chaining and the verifier") is not implemented here.
    pub fn audit_authentic(&self) -> bool {
        self.audit.len() == self.audit_signatures.len()
            && self
                .audit
                .iter()
                .zip(&self.audit_signatures)
                .all(|(r, s)| s.verify(1, "audit", &audit_bytes(r)))
    }

    /// The audit file as a reader with labels `reader` may read it: through the core's filter.
    pub fn audit_view(&self, reader: &[u64]) -> Vec<&Audit> {
        inspect::audit_view(&self.store, &self.audit, &Labels::new(reader).unwrap_or_default())
    }
}

/// A domain of the store: `account` and `labels`, as the manifest names it.
pub fn domain(account: u64, labels: &[u64]) -> Option<Domain> {
    Some(Domain::new(NonZeroU64::new(account)?, Labels::new(labels)?))
}

/// A system call by `who`; its result and notes, or why not.
fn run(k: &mut Kernel, who: Proc, call: Syscall) -> Res<(Ret, Vec<Note>)> {
    let s = k.step(&Op::Sys { pid: who.pid, tid: who.tid, call }).ok_or(Denied::Kernel(Error::Dead))?;
    match s.outcome {
        Outcome::Done(Ok(r)) => Ok((r, s.notes)),
        Outcome::Done(Err(e)) => Err(Denied::Kernel(e)),
        _ => Err(Denied::Kernel(Error::Dead)),
    }
}

fn handle(r: Ret) -> Res<u64> {
    match r {
        Ret::Handle(h) => Ok(h),
        _ => Err(Denied::Kernel(Error::Dead)),
    }
}

fn started(notes: Vec<Note>) -> Res<Proc> {
    notes
        .into_iter()
        .find_map(|n| match n {
            Note::Thread { pid, tid } => Some(Proc { pid, tid }),
            _ => None,
        })
        .ok_or(Denied::Kernel(Error::Dead))
}

/// Ideal keyd authority for signed audit records (servers/steward.md, "The audit log"). This is an
/// unforgeable symbolic token, not a cryptographic implementation. Constructing this authority
/// represents a trusted boot grant.
#[derive(Clone, Debug)]
pub struct AuditKeyd {
    signer: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditSignature {
    signer: u64,
    preimage: Vec<u8>,
}

pub const AUDIT_DOMAIN: &[u8] = b"redoubt.audit.v1\0";
fn audit_preimage(record: &[u8]) -> Vec<u8> {
    let mut bytes = AUDIT_DOMAIN.to_vec();
    bytes.extend_from_slice(&(record.len() as u64).to_le_bytes());
    bytes.extend_from_slice(record);
    bytes
}
impl AuditKeyd {
    pub fn new(signer: u64) -> Self { Self { signer } }

    /// keyd computes the preimage itself; its audit grant cannot sign another purpose.
    pub fn sign(&self, purpose: &str, record: &[u8]) -> Res<AuditSignature> {
        if purpose != "audit" {
            return Err(Denied::BadKey);
        }
        Ok(AuditSignature { signer: self.signer, preimage: audit_preimage(record) })
    }
}
impl AuditSignature {
    pub fn verify(&self, signer: u64, purpose: &str, record: &[u8]) -> bool {
        purpose == "audit" && self.signer == signer && self.preimage == audit_preimage(record)
    }

    /// Observable signed message, for the independent domain/length oracle.
    pub fn preimage(&self) -> &[u8] { &self.preimage }
}

/// Abstract serialization: every field, the record's domain included, is represented. This is
/// deliberately not a proposed audit wire/file format (which this host model does not own).
pub fn audit_bytes(a: &Audit) -> Vec<u8> { format!("{a:?}").into_bytes() }
