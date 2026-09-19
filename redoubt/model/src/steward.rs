//! The steward's milestone 1 policy, as a layer above the kernel model.
//!
//! Sources: CAPABILITIES.md (principals, agents, the powerbox and approvals), CONTAINMENT.md
//! (sessions and vaults, declassification, crash blame, the steward's own records), INIT.md
//! (milestone 1: stateless, principals from the boot manifest, `ssh approve@box`), and the owner's
//! answers (QUESTIONS 17, 18, 33-35, 48, 51, 54, 79, 80, 89-92).
//!
//! Everything here runs on the kernel model. `init` creates the server's endpoint and starts the
//! steward in the `system` budget with the `users` and `system` budgets and that endpoint; the
//! steward starts a **server** (a system-class process, an `fsd` stand-in, holding no budget)
//! receiving on it; every principal's top budget, its fixed sub-budgets (one per label set),
//! session and agent lease is a `budget_create`; every session has a **process** in its budget
//! holding its own connection to the server, narrowed to a revocation scope in the session.
//! Sessions' work is real calls to the server, and crash blame comes from the kernel's exit notices
//! of the server (CONTAINMENT.md, "Crash blame"), which the steward receives. So the kernel's
//! invariants apply to everything the policy does, and non-interference (P10, policy.rs) is judged
//! on kernel results.
//!
//! What the model leaves out: SSH itself (a login is "this key for this user name"), the
//! approval terminal (an approval channel is "a connection that authenticated with this key"), and
//! real cryptography (ids come from a keyed mixer, content hashes from FNV-1a; the real steward
//! uses a CSPRNG and a cryptographic hash).
//!
//! Policy numbers the design leaves open are constants here (`PENDING_CAP`, `DECLASSIFY_MAX`,
//! `FIELD_CAP`, `MAX_LEASE`, session sizes; README choice 23).

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::kernel::{Boot, INIT_PID, Kernel, Limits, Note, Object};
use crate::mutation::Mutation;
use crate::spec::{Counters, Error, FOREVER, WORDS};
use crate::syscall::{Message, MintSource, Op, Outcome, Ret, Syscall};

/// Pending approval requests per (account, label set) (CAPABILITIES.md; QUESTIONS 17).
pub const PENDING_CAP: usize = 4;
/// Crash blame: this many crashes blamed on one (account, label set) within `BLAME_WINDOW` log
/// out its sessions (QUESTIONS 48).
pub const BLAME_COUNT: usize = 3;
pub const BLAME_WINDOW: u64 = 10 * 60 * 1_000_000;
/// Declassification refuses items over this many bytes (CONTAINMENT.md: "a size cap").
pub const DECLASSIFY_MAX: usize = 256;
/// Every rendered free-text field is cut to this many characters.
pub const FIELD_CAP: usize = 64;
/// The longest lease: 24 hours (CAPABILITIES.md; QUESTIONS 33, 78: the steward's, not the
/// kernel's).
pub const MAX_LEASE: u64 = 24 * 3600 * 1_000_000;
/// Processes in a principal's top budget, split between its sub-budgets.
pub const PRINCIPAL_PROCESSES: u64 = 12;
/// What a login session or an agent's lease gets, carved from its parent.
pub const SESSION: Limits = Limits { pages: 40, processes: 1, weight: 5 };
pub const AGENT: Limits = Limits { pages: 40, processes: 2, weight: 4 };

/// A principal from the boot manifest (INIT.md, `principals`). Keys are abstract ids.
#[derive(Clone, Debug)]
pub struct PrincipalSpec {
    pub name: String,
    pub account: u64,
    pub login_keys: Vec<u64>,
    pub approval_keys: Vec<u64>,
    pub owned_labels: Vec<u64>,
    pub pages: u64,
    pub weight: u64,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub principals: Vec<PrincipalSpec>,
    /// Keys `keyd` holds at boot (host keys, principals' signing keys).
    pub keyd_keys: Vec<u64>,
}

/// Why the steward said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    BadManifest,
    UnknownPrincipal,
    UnknownSession,
    /// Not a key for this role, or a key `keyd` holds.
    BadKey,
    /// The principal does not own a label the operation needs.
    NotOwner,
    /// Labelled sessions may only submit requests.
    Labelled,
    /// The session's pending requests are at `PENDING_CAP`.
    Cap,
    TooBig,
    NotPrintable,
    NoSuchRequest,
    /// The approval named a different content hash.
    HashMismatch,
    NotApprover,
    /// A lease over `MAX_LEASE`, or 0.
    BadLease,
    /// The (account, label set) was logged out by crash blame; new sessions wait out the window.
    LockedOut,
    /// Only the lease's sponsor ends it.
    NotSponsor,
    Kernel(Error),
}

type Res<T> = Result<T, Denied>;

#[derive(Clone, Debug)]
pub struct Principal {
    pub spec: PrincipalSpec,
    /// The top budget (kernel id) and the steward's handle to it.
    pub budget: u64,
    pub h: u64,
    /// Fixed sub-budgets of the top budget, one per label set the principal may use (none, and
    /// each owned label), split at boot: (budget id, handle). Sessions are carved from the one for
    /// their label set, so one label set's leases never move another's free limits (QUESTIONS 89).
    pub subs: BTreeMap<Vec<u64>, (u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionKind {
    /// `ssh alice@box` or `ssh alice+X@box`.
    Login,
    /// An agent under a lease, started by the steward.
    Agent,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: u64,
    pub principal: usize,
    pub kind: SessionKind,
    /// The steward-assigned name the approval screen shows (`session-3`, `agent-7`).
    pub name: String,
    /// The session's labels; its SSH channel carries the same (CONTAINMENT.md).
    pub labels: Vec<u64>,
    pub budget: u64,
    pub h: u64,
    /// The session's process (pid), holding the server's endpoint in slot 1.
    pub pid: u64,
    /// For an agent: when its lease ends.
    pub deadline: Option<u64>,
    /// Requests this session has submitted (drives its request ids).
    pub submitted: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Start an agent with this label under the sponsor, for `lease` µs.
    AgentWithLabel { label: u64, lease: u64 },
    /// Copy vault item `item` of `label` to the unlabelled volume.
    Declassify { label: u64, item: u64 },
    /// Anything else an agent asks its sponsor for; only its text matters here.
    Note { what: String },
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: u64,
    pub session: u64,
    pub account: u64,
    /// The principal who may approve: the requester's principal.
    pub approver: usize,
    /// The requesting session's labels.
    pub labels: Vec<u64>,
    pub content: Content,
    /// Declassification: the item as it was at submission, and the reader budget it was read
    /// through.
    pub snapshot: Option<Vec<u8>>,
    pub reader: u64,
    pub reason: String,
    /// Binds the approval to exactly this content.
    pub hash: u64,
}

/// A request as the approval screen shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub id: u64,
    pub hash: u64,
    pub labels: Vec<u64>,
    pub text: String,
}

/// An `ssh approve@box` connection, authenticated with a principal's approval key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    pub principal: usize,
    pub key: u64,
}

/// The audit file: append-only, written only by the steward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Audit {
    Login {
        session: u64,
        principal: usize,
        key: u64,
        labels: Vec<u64>,
    },
    /// `parent` is the agent session it runs inside, if any (a budget id would count other
    /// principals' budgets: ids come from one counter).
    AgentStarted {
        session: u64,
        sponsor: usize,
        parent: Option<u64>,
        labels: Vec<u64>,
        deadline: u64,
    },
    Submitted {
        id: u64,
        account: u64,
        labels: Vec<u64>,
    },
    Approved {
        id: u64,
        principal: usize,
        key: u64,
        hash: u64,
        labels: Vec<u64>,
    },
    Denied {
        id: u64,
        principal: usize,
        labels: Vec<u64>,
    },
    /// `reader` is the budget the snapshot was read through (QUESTIONS 54).
    Declassified {
        id: u64,
        label: u64,
        bytes: Vec<u8>,
        reader: u64,
    },
    /// A session wrote an item of `label`.
    Wrote {
        session: u64,
        labels: Vec<u64>,
        label: u64,
    },
    Blamed {
        account: u64,
        labels: Vec<u64>,
        at: u64,
    },
    LoggedOut {
        account: u64,
        labels: Vec<u64>,
        at: u64,
    },
    LeaseEnded {
        session: u64,
        by: u64,
    },
}

impl Audit {
    /// The labels a record carries: a reader sees it only if its labels ⊇ these (QUESTIONS 92).
    pub fn labels(&self) -> &[u64] {
        match self {
            Audit::Login { labels, .. }
            | Audit::AgentStarted { labels, .. }
            | Audit::Submitted { labels, .. }
            | Audit::Approved { labels, .. }
            | Audit::Denied { labels, .. }
            | Audit::Wrote { labels, .. }
            | Audit::Blamed { labels, .. }
            | Audit::LoggedOut { labels, .. } => labels,
            Audit::Declassified { label, .. } => core::slice::from_ref(label),
            Audit::LeaseEnded { .. } => &[],
        }
    }
}

/// A process the steward runs or watches: (pid, a tid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u64,
    pub tid: u64,
}

#[derive(Clone, Debug)]
pub struct Steward {
    pub mutation: Option<Mutation>,
    pub k: Kernel,
    pub me: Proc,
    /// The server, and the steward's handles: its receive right to the server's endpoint and to
    /// the endpoint it names as every process's exit endpoint.
    pub server: Proc,
    srv: u64,
    exits: u64,
    pub principals: Vec<Principal>,
    pub keyd: BTreeSet<u64>,
    pub sessions: BTreeMap<u64, Session>,
    pub requests: BTreeMap<u64, Request>,
    /// Labelled volumes: (label, item) -> bytes.
    pub vault: BTreeMap<(u64, u64), Vec<u8>>,
    /// What declassification copied out, in order: (label, bytes).
    pub declassified: Vec<(u64, Vec<u8>)>,
    /// Blame times per (account, label set).
    pub blames: BTreeMap<(u64, Vec<u64>), Vec<u64>>,
    /// Logged-out (account, label set)s and when their lockout ends (QUESTIONS 91).
    pub locked: BTreeMap<(u64, Vec<u64>), u64>,
    pub audit: Vec<Audit>,
    /// Sessions started so far, per principal (drives session ids and names).
    started: BTreeMap<usize, u64>,
    /// The badge of the next session's connection to the server: each session gets its own
    /// (CAPABILITIES.md, one badge, one client).
    next_badge: u64,
    secret: u64,
    counter: u64,
}

/// splitmix64's finaliser: the model's stand-in for a keyed random function.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// FNV-1a over a request's exact content (the model's stand-in for a cryptographic hash).
fn content_hash(content: &Content, snapshot: &Option<Vec<u8>>, reason: &str, labels: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    match content {
        Content::AgentWithLabel { label, lease } => {
            eat(b"agent");
            eat(&label.to_le_bytes());
            eat(&lease.to_le_bytes());
        }
        Content::Declassify { label, item } => {
            eat(b"declassify");
            eat(&label.to_le_bytes());
            eat(&item.to_le_bytes());
        }
        Content::Note { what } => {
            eat(b"note");
            eat(what.as_bytes());
        }
    }
    if let Some(s) = snapshot {
        eat(s);
    }
    eat(reason.as_bytes());
    for l in labels {
        eat(&l.to_le_bytes());
    }
    h
}

/// Printable ASCII only (QUESTIONS 34: a whitelist, so bidi and format characters go too), cut
/// to `cap` characters, with quotes and backslashes escaped so a field cannot end its own quoting.
pub fn sanitize(s: &str, cap: usize, whitelist: bool) -> String {
    let mut out = String::new();
    let keep = |c: &char| if whitelist { (' '..='~').contains(c) } else { !c.is_control() };
    for c in s.chars().filter(keep).take(cap) {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A lease in human units: "2 h 5 min", "90 s", "250 ms".
pub fn human(us: u64) -> String {
    let (h, m, s, ms) = (us / 3_600_000_000, us / 60_000_000 % 60, us / 1_000_000 % 60, us / 1000 % 1000);
    match (h, m, s) {
        (0, 0, 0) => format!("{ms} ms"),
        (0, 0, s) => format!("{s} s"),
        (0, m, _) => format!("{m} min"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

/// Declassifiable: printable ASCII text and newlines.
fn printable(b: &[u8]) -> bool {
    b.iter().all(|c| (0x20..0x7f).contains(c) || *c == b'\n')
}

impl Steward {
    /// Validate the manifest, boot the kernel model, have `init` start the steward in `system`
    /// with the `users` and `system` budgets; the steward starts the server and creates each
    /// principal's top budget.
    pub fn new(manifest: &Manifest, secret: u64, mutation: Option<Mutation>) -> Res<Steward> {
        // The manifest: accounts non-zero and distinct; no key in both roles; no login or
        // approval key that keyd holds (CAPABILITIES.md, "The approval key is the person's own").
        let mut accounts = BTreeSet::new();
        let mut logins = BTreeSet::new();
        let mut approvals = BTreeSet::new();
        for p in &manifest.principals {
            if p.account == 0 || !accounts.insert(p.account) {
                return Err(Denied::BadManifest);
            }
            logins.extend(p.login_keys.iter().copied());
            approvals.extend(p.approval_keys.iter().copied());
        }
        if logins.intersection(&approvals).next().is_some()
            || manifest.keyd_keys.iter().any(|k| logins.contains(k) || approvals.contains(k))
        {
            return Err(Denied::BadManifest);
        }
        let boot = Boot {
            root: Limits { pages: 4096, processes: 64, weight: 1000 },
            system: Limits { pages: 512, processes: 8, weight: 250 },
            users: Limits { pages: 3000, processes: 48, weight: 500 },
            ..Boot::default()
        };
        let mut k = Kernel::boot(&boot, mutation).map_err(|_| Denied::BadManifest)?;
        let init = Proc { pid: INIT_PID, tid: *k.processes[&INIT_PID].threads.first().unwrap() };
        let e = handle(run(&mut k, init, Syscall::EndpointCreate)?.0)?;
        // init creates the server's endpoint (INIT.md), so its receive right is stamped `root` and
        // the steward can narrow connections to scopes anywhere (mint only narrows; README choice
        // 30).
        let srv = handle(run(&mut k, init, Syscall::EndpointCreate)?.0)?;
        // init's slots: 1 root, 2 system, 3 users. The steward gets users (1), system (2) and the
        // server's endpoint (3).
        let ph = handle(run(&mut k, init, Syscall::ProcessCreate { budget: 2, exit_endpoint: e })?.0)?;
        let start = Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: vec![3, 2, srv] };
        let me = started(run(&mut k, init, start)?.1)?;
        let mut st = Steward {
            mutation,
            k,
            me,
            server: me,
            srv: 3,
            exits: 0,
            principals: Vec::new(),
            keyd: manifest.keyd_keys.iter().copied().collect(),
            sessions: BTreeMap::new(),
            requests: BTreeMap::new(),
            vault: BTreeMap::new(),
            declassified: Vec::new(),
            blames: BTreeMap::new(),
            locked: BTreeMap::new(),
            audit: Vec::new(),
            started: BTreeMap::new(),
            next_badge: 1,
            secret,
            counter: 0,
        };
        st.exits = handle(st.sys(Syscall::EndpointCreate)?)?;
        st.start_server()?;
        for spec in &manifest.principals {
            // The steward's slot 1 is `users`.
            let l = Limits { pages: spec.pages, processes: PRINCIPAL_PROCESSES, weight: spec.weight };
            let h = st.budget_create(1, l, &[], spec.account, FOREVER)?;
            let budget = st.budget_id(h);
            // An equal share for each label set, less each sub-budget's own page.
            let sets: Vec<Vec<u64>> =
                core::iter::once(Vec::new()).chain(spec.owned_labels.iter().map(|l| vec![*l])).collect();
            let n = sets.len() as u64;
            let own = st.k.costs.budget;
            let share = Limits {
                pages: (spec.pages / n).saturating_sub(own),
                processes: PRINCIPAL_PROCESSES / n,
                weight: spec.weight / n,
            };
            let mut subs = BTreeMap::new();
            for labels in sets {
                let sh = st.budget_create(h, share, &labels, 0, FOREVER)?;
                subs.insert(labels, (st.budget_id(sh), sh));
            }
            st.principals.push(Principal { spec: spec.clone(), budget, h, subs });
        }
        Ok(st)
    }

    fn broken(&self, m: Mutation) -> bool {
        self.mutation == Some(m)
    }

    /// A system call by the steward's thread.
    fn sys(&mut self, call: Syscall) -> Res<Ret> {
        run(&mut self.k, self.me, call).map(|x| x.0)
    }

    /// Start the server: a process in `system` receiving on the server endpoint (slot 1), named
    /// with the steward's exit endpoint. INIT.md: a restarted server receives on the same endpoint.
    /// It is given no budget handle: only `init` and the steward hold system budgets (QUESTIONS 79).
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

    fn budget_create_labelled(&mut self, parent: u64, l: Limits, labels: &[u64], deadline: u64) -> Res<u64> {
        self.budget_create(parent, l, labels, 0, deadline)
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
            first: 0,
            labels: labels.to_vec(),
            account,
            deadline,
        };
        handle(self.sys(call)?)
    }

    fn budget_id(&self, h: u64) -> u64 {
        match self.k.processes[&self.me.pid].handles.get(&h).map(|x| x.object) {
            Some(Object::Budget(b)) => b,
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

    /// Housekeeping after every operation: exit notices (crash blame, a restarted server) and
    /// sessions whose budget the kernel destroyed (an expired lease), with their requests.
    pub fn poll(&mut self) {
        loop {
            match self.sys(Syscall::Receive { h: Some(self.exits), timeout: 0, max_transfer: 0 }) {
                Ok(Ret::ExitNotice { pid, blamed_account, blamed_labels, .. }) => {
                    if pid == self.server.pid {
                        // INIT.md: init passes each exit notice's blame to the steward.
                        self.blame(blamed_account, blamed_labels);
                        let _ = self.start_server();
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let k = &self.k;
        self.sessions.retain(|_, s| k.budgets.contains_key(&s.budget));
        // Requests of dead sessions are dropped (and so leave the cap).
        if !self.broken(Mutation::PolicyDeadSessionRequestsKept) {
            let sessions = &self.sessions;
            self.requests.retain(|_, r| sessions.contains_key(&r.session));
        }
    }

    pub fn tick(&mut self, dt: u64) {
        self.k.step(&Op::Tick { dt: dt.min(crate::kernel::MAX_TICK) });
        self.poll();
    }

    pub fn principal(&self, name: &str) -> Option<usize> {
        self.principals.iter().position(|p| p.spec.name == name)
    }

    /// A session: a budget under `parent` (a handle the steward holds), and a process in it
    /// holding a connection to the server: a handle with its own badge, narrowed to a revocation
    /// scope inside the session budget, so ending the session revokes it and the steward never
    /// hands a budget out (QUESTIONS 80).
    fn new_session(
        &mut self,
        principal: usize,
        parent: u64,
        kind: SessionKind,
        labels: Vec<u64>,
        l: Limits,
        deadline: u64,
    ) -> Res<u64> {
        let h = self.budget_create(parent, l, &labels, 0, deadline)?;
        let budget = self.budget_id(h);
        let scope = self.budget_create(h, Limits { pages: 0, processes: 0, weight: 0 }, &labels, 0, FOREVER);
        let narrow = if self.broken(Mutation::PolicyNarrowToSessionBudget) { Ok(h) } else { scope };
        let process = self.sys(Syscall::ProcessCreate { budget: h, exit_endpoint: self.exits });
        let badge = self.next_badge;
        self.next_badge += 1;
        let mint = match narrow {
            Ok(sh) => {
                self.sys(Syscall::Mint { source: MintSource::Handle(self.srv), badge, budget: Some(sh) })
            }
            Err(e) => Err(e),
        };
        let (ph, mh) = match (process, mint) {
            (Ok(Ret::Handle(ph)), Ok(Ret::Handle(mh))) => (ph, mh),
            (p, m) => {
                let _ = self.sys(Syscall::BudgetDestroy { h });
                return Err(p.and(m).err().unwrap_or(Denied::Kernel(Error::Dead)));
            }
        };
        let start = Syscall::ProcessStart { process: ph, entry: 0, sp: 0, arg: 0, handles: vec![mh] };
        let pid = run(&mut self.k, self.me, start).and_then(|(_, n)| started(n)).map(|p| p.pid);
        let _ = self.sys(Syscall::HandleClose { h: mh });
        let _ = self.sys(Syscall::HandleClose { h: ph });
        let pid = match pid {
            Ok(pid) => pid,
            Err(e) => {
                let _ = self.sys(Syscall::BudgetDestroy { h });
                return Err(e);
            }
        };
        // A random id and a per-principal name: nothing another principal can count
        // (QUESTIONS 18; README choice 22).
        let n = self.started.entry(principal).or_insert(0);
        *n += 1;
        let name = format!("{}-{}", if kind == SessionKind::Agent { "agent" } else { "session" }, n);
        let mut id = mix(self.secret ^ 0x5e55 ^ mix(((principal as u64) << 32) ^ *n));
        while id == 0 || self.sessions.contains_key(&id) {
            id = mix(id);
        }
        let deadline = if deadline == FOREVER { None } else { Some(deadline) };
        self.sessions.insert(
            id,
            Session { id, principal, kind, name, labels, budget, h, pid, deadline, submitted: 0 },
        );
        Ok(id)
    }

    // -------------------------------------------------------------------------------------------
    // Sessions.

    /// `ssh name@box` (`label` none) or `ssh name+X@box`: the key must be one of the principal's
    /// login keys and not one `keyd` holds; a vault session carries exactly the one label, which
    /// the principal must own.
    pub fn login(&mut self, name: &str, label: Option<u64>, key: u64) -> Res<u64> {
        let p = self.principal(name).ok_or(Denied::UnknownPrincipal)?;
        let spec = &self.principals[p].spec;
        // sshd rejects authentication with any key keyd holds (CAPABILITIES.md): otherwise a
        // hijacked session could log in over loopback by asking keyd to sign.
        let held_by_keyd = self.keyd.contains(&key) && !self.broken(Mutation::PolicyLoginWithKeydKey);
        if !spec.login_keys.contains(&key) || held_by_keyd {
            return Err(Denied::BadKey);
        }
        let labels = match label {
            None => Vec::new(),
            Some(x) => {
                if !spec.owned_labels.contains(&x) && !self.broken(Mutation::PolicyVaultWithoutOwnership) {
                    return Err(Denied::NotOwner);
                }
                vec![x]
            }
        };
        self.not_locked(p, &labels)?;
        let parent = self.sub(p, &labels);
        let id = self.new_session(p, parent, SessionKind::Login, labels.clone(), SESSION, FOREVER)?;
        self.audit.push(Audit::Login { session: id, principal: p, key, labels });
        Ok(id)
    }

    /// The handle of principal `p`'s sub-budget for `labels` (QUESTIONS 89).
    fn sub(&self, p: usize, labels: &[u64]) -> u64 {
        let set = if self.broken(Mutation::PolicyCarveFromUnlabelled) { &[][..] } else { labels };
        // A label set with no sub-budget cannot happen (only owned labels get sessions); the
        // unlabelled one stands in.
        let subs = &self.principals[p].subs;
        subs.get(set).or_else(|| subs.get(&Vec::new())).map_or(self.principals[p].h, |s| s.1)
    }

    /// New sessions of a logged-out (account, label set) are refused until its window passes
    /// (QUESTIONS 91).
    fn not_locked(&self, p: usize, labels: &[u64]) -> Res<()> {
        let key = (self.principals[p].spec.account, labels.to_vec());
        match self.locked.get(&key) {
            Some(until) if self.k.now < *until && !self.broken(Mutation::PolicyNoLockout) => {
                Err(Denied::LockedOut)
            }
            _ => Ok(()),
        }
    }

    /// The session ends (its user logs out); everything in its budget goes with it.
    pub fn end_session(&mut self, session: u64) -> Res<()> {
        let s = self.sessions.remove(&session).ok_or(Denied::UnknownSession)?;
        self.sys(Syscall::BudgetDestroy { h: s.h })?;
        Ok(())
    }

    /// Whether `lease` is one the steward grants (QUESTIONS 33: at most `MAX_LEASE`).
    fn lease_ok(&self, lease: u64) -> bool {
        lease > 0 && (lease <= MAX_LEASE || self.broken(Mutation::PolicyUnboundedLease))
    }

    /// An unlabelled session starts an agent on a lease: a budget with a deadline. A login
    /// session's agent is carved from its principal's budget; an agent's sub-agent from the
    /// agent's own budget, its lease ending no later than the agent's (QUESTIONS 33). Labelled
    /// sessions may only submit requests (a labelled agent needs an approval).
    pub fn start_agent(&mut self, session: u64, lease: u64) -> Res<u64> {
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?.clone();
        if !s.labels.is_empty() {
            return Err(Denied::Labelled);
        }
        if !self.lease_ok(lease) {
            return Err(Denied::BadLease);
        }
        self.not_locked(s.principal, &[])?;
        let mut deadline = self.k.now.saturating_add(lease);
        let (parent, parent_session, limits) =
            if s.kind == SessionKind::Agent && !self.broken(Mutation::PolicySubAgentOutlivesAgent) {
                deadline = deadline.min(s.deadline.unwrap_or(FOREVER));
                let free =
                    self.k.budgets.get(&s.budget).map_or(0, |b| b.pages_limit.saturating_sub(b.pages_used));
                (s.h, Some(s.id), Limits { pages: free / 2, processes: 1, weight: 1 })
            } else {
                (self.sub(s.principal, &[]), None, AGENT)
            };
        let id = self.new_session(s.principal, parent, SessionKind::Agent, Vec::new(), limits, deadline)?;
        let audit = Audit::AgentStarted {
            session: id,
            sponsor: s.principal,
            parent: parent_session,
            labels: Vec::new(),
            deadline,
        };
        self.audit.push(audit);
        Ok(id)
    }

    /// A session writes item `item` of label `label`'s volume. Every write needs equal labels
    /// (QUESTIONS 51: no write down, and no blind write up), so only a vault session carrying
    /// exactly that label writes there.
    pub fn write_item(&mut self, session: u64, label: u64, item: u64, bytes: Vec<u8>) -> Res<()> {
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?;
        let allowed = if self.broken(Mutation::PolicyWriteUp) {
            s.labels.iter().all(|l| *l == label)
        } else {
            s.labels == [label]
        };
        if !allowed {
            return Err(Denied::NotOwner);
        }
        let labels = s.labels.clone();
        self.vault.insert((label, item), bytes);
        self.audit.push(Audit::Wrote { session, labels, label });
        Ok(())
    }

    /// Read item `item` of `label` through a short-lived reader budget carrying exactly that
    /// label, which the steward creates and destroys (QUESTIONS 54): the steward itself stays
    /// unlabelled. Returns the bytes and the reader's budget id. The model reads the volume
    /// directly; the reader budget stands for the process that would.
    fn read_item(&mut self, label: u64, item: u64) -> Res<(Vec<u8>, u64)> {
        let bytes = self.vault.get(&(label, item)).cloned().unwrap_or_default();
        if self.broken(Mutation::PolicyDeclassifyWithoutReader) {
            return Ok((bytes, 0));
        }
        // Under `users` (the steward's slot 1): one page for its own object, a deadline.
        let l = Limits { pages: self.k.costs.budget, processes: 0, weight: 0 };
        let deadline = self.k.now.saturating_add(crate::spec::SLICE);
        let h = self.budget_create_labelled(1, l, &[label], deadline)?;
        let reader = self.budget_id(h);
        self.sys(Syscall::BudgetDestroy { h })?;
        Ok((bytes, reader))
    }

    // -------------------------------------------------------------------------------------------
    // Sessions' work on the kernel: calls to the server, and the server's side.

    /// The session's process calls the server (no timeout), first starting another thread so it
    /// can call again while this call waits. The result is what the calling thread got now.
    pub fn work(&mut self, session: u64) -> Res<Outcome> {
        let pid = self.sessions.get(&session).ok_or(Denied::UnknownSession)?.pid;
        let Some(tid) = self.k.runnable().into_iter().find(|(p, _)| *p == pid).map(|x| x.1) else {
            return Ok(Outcome::Blocked);
        };
        let who = Proc { pid, tid };
        let _ = self.k.step(&Op::Sys { pid, tid, call: Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 } });
        let call = Syscall::Call {
            h: 1,
            words: [session; WORDS],
            handles: Vec::new(),
            lend: None,
            timeout: FOREVER,
        };
        Ok(self.k.step(&Op::Sys { pid: who.pid, tid: who.tid, call }).map_or(Outcome::Gone, |s| s.outcome))
    }

    /// The server answers the calls it holds open (from `hold`), then takes what is waiting and
    /// answers all of it, until nothing is left. An abandoned call is answered too: that frees it.
    pub fn serve(&mut self) {
        let s = self.server;
        let reply = |k: &mut Kernel, m: u64| {
            let call = Syscall::Reply { msg_id: m, words: [0; WORDS], handles: Vec::new() };
            k.step(&Op::Sys { pid: s.pid, tid: s.tid, call });
        };
        let held: Vec<u64> = self.k.threads.get(&s.tid).map_or(Vec::new(), |t| {
            t.serving.iter().filter_map(|m| self.k.msgs.get(m)).map(|m| m.rid).collect()
        });
        for m in held {
            reply(&mut self.k, m);
        }
        for _ in 0..4096 {
            let recv = Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 };
            match self.k.step(&Op::Sys { pid: s.pid, tid: s.tid, call: recv }).map(|x| x.outcome) {
                Some(Outcome::Done(Ok(Ret::Message(m)))) if m.kind == crate::syscall::MsgKind::Call => {
                    reply(&mut self.k, m.msg_id)
                }
                Some(Outcome::Done(Ok(Ret::Message(_)))) => {}
                Some(Outcome::Done(Ok(Ret::Abandoned { .. }))) => {}
                _ => break,
            }
        }
    }

    /// The server takes one waiting call and keeps it open (it is working on it: its current call);
    /// an abandoned-call notice on the way is answered with a reply, which frees the call.
    pub fn hold(&mut self) -> Option<Message> {
        let s = self.server;
        let recv = Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 };
        loop {
            match self.k.step(&Op::Sys { pid: s.pid, tid: s.tid, call: recv.clone() }).map(|x| x.outcome) {
                Some(Outcome::Done(Ok(Ret::Message(m)))) => return Some(m),
                Some(Outcome::Done(Ok(Ret::Abandoned { msg_id }))) => {
                    let call = Syscall::Reply { msg_id, words: [0; WORDS], handles: Vec::new() };
                    self.k.step(&Op::Sys { pid: s.pid, tid: s.tid, call });
                }
                _ => return None,
            }
        }
    }

    /// The server crashes (the kernel reports the account of the message it was serving).
    pub fn crash_server(&mut self) {
        let s = self.server;
        self.k.step(&Op::Fault { pid: s.pid, tid: s.tid });
        self.poll();
    }

    // -------------------------------------------------------------------------------------------
    // The powerbox.

    /// Pending requests of one session.
    pub fn pending_by(&self, session: u64) -> usize {
        self.requests.values().filter(|r| r.session == session).count()
    }

    /// A session's fair share of its bucket: the cap divided among the bucket's live sessions,
    /// at least one (QUESTIONS 90).
    pub fn share(&self, session: u64) -> usize {
        let Some(s) = self.sessions.get(&session) else { return 0 };
        let account = self.principals[s.principal].spec.account;
        let n = self
            .sessions
            .values()
            .filter(|x| self.principals[x.principal].spec.account == account && x.labels == s.labels)
            .count();
        (PENDING_CAP / n.max(1)).max(1)
    }

    /// Pending requests of the requester's (account, label set) (QUESTIONS 17).
    fn pending_of(&self, account: u64, labels: &[u64]) -> usize {
        let per_label_set = !self.broken(Mutation::PolicyCapPerAccount);
        self.requests
            .values()
            .filter(|r| r.account == account && (!per_label_set || r.labels == labels))
            .count()
    }

    /// Submit a request. The approver is the requester's principal; a labelled request is
    /// refused unless the approver owns every label on it; a declassification snapshots the item
    /// now. Each (account, label set) has at most `PENDING_CAP` pending requests, and each session
    /// at most its fair share of them (QUESTIONS 90).
    pub fn submit(&mut self, session: u64, content: Content, reason: &str) -> Res<u64> {
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?.clone();
        let approver = s.principal;
        let owned = &self.principals[approver].spec.owned_labels;
        let account = self.principals[approver].spec.account;
        if !s.labels.iter().all(|l| owned.contains(l)) {
            return Err(Denied::NotOwner);
        }
        let mut reader = 0;
        let snapshot = match &content {
            Content::AgentWithLabel { label, .. } if !owned.contains(label) => return Err(Denied::NotOwner),
            Content::AgentWithLabel { lease, .. } if !self.lease_ok(*lease) => return Err(Denied::BadLease),
            Content::Declassify { label, item } => {
                // Only the label's owner declassifies, and only from a session carrying it.
                if !owned.contains(label) || !s.labels.contains(label) {
                    return Err(Denied::NotOwner);
                }
                let (bytes, r) = self.read_item(*label, *item)?;
                reader = r;
                if bytes.len() > DECLASSIFY_MAX {
                    return Err(Denied::TooBig);
                }
                if !printable(&bytes) {
                    return Err(Denied::NotPrintable);
                }
                Some(bytes)
            }
            _ => None,
        };
        if self.pending_of(account, &s.labels) >= PENDING_CAP && !self.broken(Mutation::PolicyNoPendingCap) {
            return Err(Denied::Cap);
        }
        if self.pending_by(session) >= self.share(session) && !self.broken(Mutation::PolicyNoFairShare) {
            return Err(Denied::Cap);
        }
        // A random 64-bit id: nothing another session can observe or predict.
        let id = if self.broken(Mutation::PolicySequentialIds) {
            self.counter += 1;
            self.counter
        } else {
            let mut n = s.submitted;
            loop {
                let id = mix(self.secret ^ mix(session.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n));
                if !self.requests.contains_key(&id) && id != 0 {
                    break id;
                }
                n += 1 << 32;
            }
        };
        self.sessions.get_mut(&session).unwrap().submitted += 1;
        let snapshot = if self.broken(Mutation::PolicyDeclassifyLive) { None } else { snapshot };
        let hash = content_hash(&content, &snapshot, reason, &s.labels);
        self.requests.insert(
            id,
            Request {
                id,
                session,
                account,
                approver,
                labels: s.labels.clone(),
                content,
                snapshot,
                reader,
                reason: String::from(reason),
                hash,
            },
        );
        self.audit.push(Audit::Submitted { id, account, labels: s.labels.clone() });
        Ok(id)
    }

    /// What `approver` sees on its approval screen: its own principal's requests, and a labelled
    /// one only if it owns every label on it. Rendered by the steward from the structured request:
    /// who asks (the session's kind and steward-assigned name), what, for how long in human units;
    /// the requester's reason quoted, escaped, capped and marked untrusted; printable ASCII only.
    pub fn pending(&self, approver: usize) -> Vec<Rendered> {
        let owned = &self.principals[approver].spec.owned_labels;
        let wl = !self.broken(Mutation::PolicyRenderNotWhitelisted);
        let mut out = Vec::new();
        for r in self.requests.values() {
            let visible = r.approver == approver && r.labels.iter().all(|l| owned.contains(l));
            if !visible && !self.broken(Mutation::PolicyShowLabelledToAll) {
                continue;
            }
            let who = sanitize(&self.principals[r.approver].spec.name, FIELD_CAP, wl);
            let free_text_withheld =
                !r.labels.is_empty() && !self.broken(Mutation::PolicyLabelledFreeTextShown);
            let session = self.sessions.get(&r.session).map_or(String::from("(ended)"), |s| s.name.clone());
            let what = match &r.content {
                Content::AgentWithLabel { label, lease } => {
                    format!("start an agent labelled {label} for {}", human(*lease))
                }
                Content::Declassify { label, item } => {
                    let shown = r
                        .snapshot
                        .as_ref()
                        .map(|b| sanitize(&String::from_utf8_lossy(b), DECLASSIFY_MAX, wl));
                    format!("declassify item {item} of label {label}: \"{}\"", shown.unwrap_or_default())
                }
                // A labelled requester's free text is not shown (QUESTIONS 35): it could carry
                // the vault out; only declassification may.
                Content::Note { .. } if free_text_withheld => {
                    String::from("a note (text withheld: labelled session)")
                }
                Content::Note { what } => {
                    format!("a note (untrusted): \"{}\"", sanitize(what, FIELD_CAP, wl))
                }
            };
            let reason = if free_text_withheld {
                String::from("reason withheld (labelled session)")
            } else {
                format!("reason (untrusted): \"{}\"", sanitize(&r.reason, FIELD_CAP, wl))
            };
            let kind = self
                .sessions
                .get(&r.session)
                .map_or("session", |s| if s.kind == SessionKind::Agent { "agent" } else { "session" });
            let text =
                format!("request from {who} ({kind} {session}, labels {:?}): {what}; {reason}", r.labels);
            out.push(Rendered { id: r.id, hash: r.hash, labels: r.labels.clone(), text });
        }
        out
    }

    /// `ssh approve@box`: only with one of the principal's approval keys, never a login key or
    /// one `keyd` holds.
    pub fn open_approval(&self, name: &str, key: u64) -> Res<Channel> {
        let p = self.principal(name).ok_or(Denied::UnknownPrincipal)?;
        let spec = &self.principals[p].spec;
        if !spec.approval_keys.contains(&key) || spec.login_keys.contains(&key) || self.keyd.contains(&key) {
            return Err(Denied::BadKey);
        }
        Ok(Channel { principal: p, key })
    }

    /// Approve request `id` whose content hash is `hash`, on an approval channel. The request must
    /// be the channel principal's to approve; the approval grants no more than the approver holds.
    pub fn approve(&mut self, ch: Channel, id: u64, hash: u64) -> Res<()> {
        let r = self.requests.get(&id).ok_or(Denied::NoSuchRequest)?.clone();
        if r.approver != ch.principal {
            return Err(Denied::NotApprover);
        }
        if r.hash != hash && !self.broken(Mutation::PolicyApproveIgnoresHash) {
            return Err(Denied::HashMismatch);
        }
        // The approval grants no more than the approver holds: every label on the request, and
        // the label it asks for.
        let owned = self.principals[ch.principal].spec.owned_labels.clone();
        let asked = match r.content {
            Content::AgentWithLabel { label, .. } | Content::Declassify { label, .. } => Some(label),
            Content::Note { .. } => None,
        };
        if !r.labels.iter().chain(asked.iter()).all(|l| owned.contains(l)) {
            return Err(Denied::NotOwner);
        }
        if let Content::AgentWithLabel { label, .. } = r.content {
            self.not_locked(ch.principal, &[label])?;
        }
        self.requests.remove(&id);
        self.audit.push(Audit::Approved {
            id,
            principal: ch.principal,
            key: ch.key,
            hash,
            labels: r.labels.clone(),
        });
        match r.content {
            Content::AgentWithLabel { label, lease } => {
                let deadline = self.k.now.saturating_add(lease);
                let parent = self.sub(ch.principal, &[label]);
                let sid =
                    self.new_session(ch.principal, parent, SessionKind::Agent, vec![label], AGENT, deadline)?;
                let audit = Audit::AgentStarted {
                    session: sid,
                    sponsor: ch.principal,
                    parent: None,
                    labels: vec![label],
                    deadline,
                };
                self.audit.push(audit);
            }
            Content::Declassify { label, item } => {
                let bytes = match r.snapshot {
                    Some(s) => s,
                    None => self.vault.get(&(label, item)).cloned().unwrap_or_default(),
                };
                self.declassified.push((label, bytes.clone()));
                self.audit.push(Audit::Declassified { id, label, bytes, reader: r.reader });
            }
            Content::Note { .. } => {}
        }
        Ok(())
    }

    pub fn deny(&mut self, ch: Channel, id: u64) -> Res<()> {
        let r = self.requests.get(&id).ok_or(Denied::NoSuchRequest)?;
        if r.approver != ch.principal {
            return Err(Denied::NotApprover);
        }
        let labels = r.labels.clone();
        self.requests.remove(&id);
        self.audit.push(Audit::Denied { id, principal: ch.principal, labels });
        Ok(())
    }

    /// Session `by` ends agent session `lease`. Accepted whenever `by` is a session of the lease's
    /// sponsor, ahead of any admission: an agent filling its sponsor's buckets cannot keep its
    /// sponsor from ending it (QUESTIONS 90).
    pub fn end_lease(&mut self, by: u64, lease: u64) -> Res<()> {
        let b = self.sessions.get(&by).ok_or(Denied::UnknownSession)?;
        let l = self.sessions.get(&lease).ok_or(Denied::UnknownSession)?;
        if l.kind != SessionKind::Agent || b.principal != l.principal || !b.labels.is_empty() {
            return Err(Denied::NotSponsor);
        }
        let account = self.principals[b.principal].spec.account;
        if self.broken(Mutation::PolicyEndLeaseAdmitted) && self.pending_of(account, &[]) >= PENDING_CAP {
            return Err(Denied::Cap);
        }
        self.end_session(lease)?;
        self.audit.push(Audit::LeaseEnded { session: lease, by });
        Ok(())
    }

    /// The audit file as a reader with labels `reader` may read it: the records whose labels it
    /// holds (QUESTIONS 92).
    pub fn audit_view(&self, reader: &[u64]) -> Vec<&Audit> {
        let all = self.broken(Mutation::PolicyAuditUnfiltered);
        self.audit.iter().filter(|a| all || crate::spec::superset(reader, a.labels())).collect()
    }

    // -------------------------------------------------------------------------------------------
    // Crash blame (CONTAINMENT.md): from the kernel's exit notices of the server (`poll`).

    /// Three crashes blamed on one (account, label set) within ten minutes log out that account's
    /// sessions with that label set, and their agents, and refuse new ones until the window has
    /// passed (QUESTIONS 48, 91: keyed by label set too, so a vault crashing a shared server cannot
    /// log out its owner's unlabelled sessions). Account 0 (nothing being served) blames nobody.
    fn blame(&mut self, account: u64, labels: Vec<u64>) {
        if account == 0 {
            return;
        }
        let now = self.k.now;
        self.audit.push(Audit::Blamed { account, labels: labels.clone(), at: now });
        let window = !self.broken(Mutation::PolicyBlameNoWindow);
        let per_account = self.broken(Mutation::PolicyBlamePerAccount);
        let key = (account, if per_account { Vec::new() } else { labels.clone() });
        let times = self.blames.entry(key).or_default();
        times.push(now);
        if window {
            times.retain(|t| now - *t < BLAME_WINDOW);
        }
        if times.len() >= BLAME_COUNT {
            times.clear();
            let doomed: Vec<u64> = self
                .sessions
                .values()
                .filter(|s| self.principals[s.principal].spec.account == account)
                .filter(|s| per_account || s.labels == labels)
                .map(|s| s.id)
                .collect();
            for s in doomed {
                let _ = self.end_session(s);
            }
            self.locked.insert((account, labels.clone()), now.saturating_add(BLAME_WINDOW));
            self.audit.push(Audit::LoggedOut { account, labels, at: now });
        }
    }

    pub fn keyd_add(&mut self, key: u64) {
        self.keyd.insert(key);
    }
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
