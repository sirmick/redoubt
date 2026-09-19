//! The steward's milestone 1 policy, as a layer above the kernel model.
//!
//! Sources: CAPABILITIES.md (principals, agents, the powerbox and approvals), CONTAINMENT.md
//! (sessions and vaults, declassification, crash blame, the steward's own records), INIT.md
//! (milestone 1: stateless, principals from the boot manifest, `ssh approve@box`).
//!
//! The steward here is a real process of the kernel model: `init` creates it in the `system`
//! budget and hands it the `users` budget, and every budget it makes (a principal's top budget,
//! a session, an agent's lease) is a `budget_create` it calls, so the kernel's invariants apply
//! to everything the policy does. What the model leaves out: SSH itself (a login is "this key
//! for this user name"), the approval terminal (an approval channel is "a connection that
//! authenticated with this key"), processes inside sessions, and real cryptography (request
//! ids come from a keyed mixer, content hashes from FNV-1a; the real steward uses a CSPRNG and a
//! cryptographic hash).
//!
//! Policy numbers the design leaves open are constants here (`PENDING_CAP`, `DECLASSIFY_MAX`,
//! `FIELD_CAP`, session sizes); CAPABILITIES.md and CONTAINMENT.md fix only the blame rule's
//! numbers (three crashes, ten minutes).

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::kernel::{Boot, INIT_PID, Kernel, Note, Object};
use crate::mutation::Mutation;
use crate::spec::{Class, Counters, Error, FOREVER};
use crate::syscall::{Op, Outcome, Ret, Syscall};

/// Pending approval requests per account (CAPABILITIES.md: "each account has a cap").
pub const PENDING_CAP: usize = 4;
/// Crash blame: this many crashes blamed on one account within `BLAME_WINDOW` log it out.
pub const BLAME_COUNT: usize = 3;
pub const BLAME_WINDOW: u64 = 10 * 60 * 1_000_000;
/// Declassification refuses items over this many bytes (CONTAINMENT.md: "a size cap").
pub const DECLASSIFY_MAX: usize = 256;
/// Every rendered free-text field is cut to this many characters.
pub const FIELD_CAP: usize = 64;
/// What a login session or an agent's lease gets, carved from the principal's top budget.
pub const SESSION_PAGES: u64 = 12;
pub const AGENT_PAGES: u64 = 8;

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
    /// The account's pending requests are at `PENDING_CAP`.
    Cap,
    TooBig,
    NotPrintable,
    NoSuchRequest,
    /// The approval named a different content hash.
    HashMismatch,
    NotApprover,
    Kernel(Error),
}

type Res<T> = Result<T, Denied>;

#[derive(Clone, Debug)]
pub struct Principal {
    pub spec: PrincipalSpec,
    /// The top budget (kernel id) and the steward's handle to it.
    pub budget: u64,
    pub h: u64,
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
    /// The session's labels; its SSH channel carries the same (CONTAINMENT.md).
    pub labels: Vec<u64>,
    pub budget: u64,
    pub h: u64,
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
    /// Declassification: the item as it was at submission.
    pub snapshot: Option<Vec<u8>>,
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
    Login { session: u64, principal: usize, key: u64, labels: Vec<u64> },
    AgentStarted { session: u64, sponsor: usize, labels: Vec<u64>, deadline: u64 },
    Submitted { id: u64, account: u64 },
    Approved { id: u64, principal: usize, key: u64, hash: u64 },
    Denied { id: u64, principal: usize },
    Declassified { id: u64, label: u64, bytes: Vec<u8> },
    Blamed { account: u64, at: u64 },
    LoggedOut { account: u64, at: u64 },
}

#[derive(Clone, Debug)]
pub struct Steward {
    pub mutation: Option<Mutation>,
    pub k: Kernel,
    pub pid: u64,
    pub tid: u64,
    pub principals: Vec<Principal>,
    pub keyd: BTreeSet<u64>,
    pub sessions: BTreeMap<u64, Session>,
    pub requests: BTreeMap<u64, Request>,
    /// Labelled volumes: (label, item) -> bytes.
    pub vault: BTreeMap<(u64, u64), Vec<u8>>,
    /// What declassification copied out, in order: (label, bytes).
    pub declassified: Vec<(u64, Vec<u8>)>,
    pub blames: BTreeMap<u64, Vec<u64>>,
    pub audit: Vec<Audit>,
    next_session: u64,
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

/// Printable text only: control characters stripped, cut to `cap` characters, and quotes and
/// backslashes escaped so a field cannot end its own quoting.
pub fn sanitize(s: &str, cap: usize) -> String {
    let mut out = String::new();
    for c in s.chars().filter(|c| !c.is_control()).take(cap) {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Declassifiable: printable ASCII text and newlines.
fn printable(b: &[u8]) -> bool {
    b.iter().all(|c| (0x20..0x7f).contains(c) || *c == b'\n')
}

impl Steward {
    /// Validate the manifest, boot the kernel model, have `init` start the steward in `system`
    /// with the `users` budget, and create each principal's top budget.
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
        let boot = Boot { users_pages: 600, users_processes: 12, ..Boot::default() };
        let mut k = Kernel::boot(&boot, mutation);
        let init_tid = *k.processes[&INIT_PID].threads.first().unwrap();
        let init = |k: &mut Kernel, call: Syscall| -> Res<(Ret, Vec<Note>)> {
            let s =
                k.step(&Op::Sys { pid: INIT_PID, tid: init_tid, call }).ok_or(Denied::Kernel(Error::Dead))?;
            match s.outcome {
                Outcome::Done(Ok(r)) => Ok((r, s.notes)),
                Outcome::Done(Err(e)) => Err(Denied::Kernel(e)),
                _ => Err(Denied::Kernel(Error::Dead)),
            }
        };
        let (Ret::Handle(e), _) = init(&mut k, Syscall::EndpointCreate)? else { unreachable!() };
        // init's slots: 1 root, 2 system, 3 users.
        let (Ret::Handle(ph), _) = init(&mut k, Syscall::ProcessCreate { budget: 2, exit_endpoint: e })?
        else {
            unreachable!()
        };
        let (_, notes) =
            init(&mut k, Syscall::ProcessStart { process: ph, entry: 0, sp: 0, handles: vec![3] })?;
        let Some(Note::Thread { pid, tid }) = notes.first().cloned() else {
            return Err(Denied::Kernel(Error::Dead));
        };
        let mut st = Steward {
            mutation,
            k,
            pid,
            tid,
            principals: Vec::new(),
            keyd: manifest.keyd_keys.iter().copied().collect(),
            sessions: BTreeMap::new(),
            requests: BTreeMap::new(),
            vault: BTreeMap::new(),
            declassified: Vec::new(),
            blames: BTreeMap::new(),
            audit: Vec::new(),
            next_session: 1,
            secret,
            counter: 0,
        };
        for spec in &manifest.principals {
            // The steward's slot 1 is `users`.
            let h = st.budget_create(1, spec.pages, 4, spec.weight, &[], spec.account, FOREVER)?;
            let budget = st.budget_id(h);
            st.principals.push(Principal { spec: spec.clone(), budget, h });
        }
        Ok(st)
    }

    fn broken(&self, m: Mutation) -> bool {
        self.mutation == Some(m)
    }

    /// A system call by the steward's thread.
    fn sys(&mut self, call: Syscall) -> Res<Ret> {
        let op = Op::Sys { pid: self.pid, tid: self.tid, call };
        match self.k.step(&op).map(|s| s.outcome) {
            Some(Outcome::Done(Ok(r))) => Ok(r),
            Some(Outcome::Done(Err(e))) => Err(Denied::Kernel(e)),
            _ => Err(Denied::Kernel(Error::Dead)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn budget_create(
        &mut self,
        parent: u64,
        pages: u64,
        processes: u64,
        weight: u64,
        labels: &[u64],
        account: u64,
        deadline: u64,
    ) -> Res<u64> {
        let call = Syscall::BudgetCreate {
            parent,
            pages,
            processes,
            weight,
            class: Class::User.raw(),
            labels: labels.to_vec(),
            account,
            deadline,
        };
        match self.sys(call)? {
            Ret::Handle(h) => Ok(h),
            _ => Err(Denied::Kernel(Error::Dead)),
        }
    }

    fn budget_id(&self, h: u64) -> u64 {
        match self.k.processes[&self.pid].handles.get(&h).map(|x| x.object) {
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

    /// Sessions whose budget the kernel destroyed (a lease that expired) are gone.
    fn reconcile(&mut self) {
        let k = &self.k;
        self.sessions.retain(|_, s| k.budgets.contains_key(&s.budget));
    }

    pub fn tick(&mut self, dt: u64) {
        self.k.step(&Op::Tick { dt });
        self.reconcile();
    }

    pub fn principal(&self, name: &str) -> Option<usize> {
        self.principals.iter().position(|p| p.spec.name == name)
    }

    fn new_session(
        &mut self,
        principal: usize,
        kind: SessionKind,
        labels: Vec<u64>,
        pages: u64,
        deadline: u64,
    ) -> Res<u64> {
        let parent = self.principals[principal].h;
        let h = self.budget_create(parent, pages, 1, 5, &labels, 0, deadline)?;
        let budget = self.budget_id(h);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(id, Session { id, principal, kind, labels, budget, h, submitted: 0 });
        Ok(id)
    }

    // -------------------------------------------------------------------------------------------
    // Sessions.

    /// `ssh name@box` (`label` none) or `ssh name+X@box`: the key must be one of the principal's
    /// login keys and not one `keyd` holds; a vault session carries exactly the one label, which
    /// the principal must own.
    pub fn login(&mut self, name: &str, label: Option<u64>, key: u64) -> Res<u64> {
        self.reconcile();
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
        let id = self.new_session(p, SessionKind::Login, labels.clone(), SESSION_PAGES, FOREVER)?;
        self.audit.push(Audit::Login { session: id, principal: p, key, labels });
        Ok(id)
    }

    /// The session ends (its user logs out); everything in its budget goes with it.
    pub fn end_session(&mut self, session: u64) -> Res<()> {
        self.reconcile();
        let s = self.sessions.remove(&session).ok_or(Denied::UnknownSession)?;
        self.sys(Syscall::BudgetDestroy { h: s.h })?;
        Ok(())
    }

    /// An unlabelled session starts an agent under its principal, on a lease: a budget with a
    /// deadline. Labelled sessions may only submit requests (a labelled agent needs an approval).
    pub fn start_agent(&mut self, session: u64, lease: u64) -> Res<u64> {
        self.reconcile();
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?;
        if !s.labels.is_empty() {
            return Err(Denied::Labelled);
        }
        let p = s.principal;
        let deadline = self.k.now.saturating_add(lease.max(1));
        let id = self.new_session(p, SessionKind::Agent, Vec::new(), AGENT_PAGES, deadline)?;
        self.audit.push(Audit::AgentStarted { session: id, sponsor: p, labels: Vec::new(), deadline });
        Ok(id)
    }

    /// A vault session writes an item in its label's volume (no write down: only its own label).
    pub fn write_item(&mut self, session: u64, item: u64, bytes: Vec<u8>) -> Res<()> {
        self.reconcile();
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?;
        let [label] = s.labels[..] else { return Err(Denied::NotOwner) };
        self.vault.insert((label, item), bytes);
        Ok(())
    }

    // -------------------------------------------------------------------------------------------
    // The powerbox.

    fn pending_of(&self, account: u64) -> usize {
        self.requests.values().filter(|r| r.account == account).count()
    }

    /// Submit a request. The approver is the requester's principal; a labelled request is
    /// refused unless the approver owns every label on it; a declassification snapshots the item
    /// now. Each account has at most `PENDING_CAP` pending requests.
    pub fn submit(&mut self, session: u64, content: Content, reason: &str) -> Res<u64> {
        self.reconcile();
        let s = self.sessions.get(&session).ok_or(Denied::UnknownSession)?.clone();
        let approver = s.principal;
        let owned = &self.principals[approver].spec.owned_labels;
        let account = self.principals[approver].spec.account;
        if !s.labels.iter().all(|l| owned.contains(l)) {
            return Err(Denied::NotOwner);
        }
        let snapshot = match &content {
            Content::AgentWithLabel { label, .. } if !owned.contains(label) => return Err(Denied::NotOwner),
            Content::Declassify { label, item } => {
                // Only the label's owner declassifies, and only from a session carrying it.
                if !owned.contains(label) || !s.labels.contains(label) {
                    return Err(Denied::NotOwner);
                }
                let bytes = self.vault.get(&(*label, *item)).cloned().unwrap_or_default();
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
        if self.pending_of(account) >= PENDING_CAP && !self.broken(Mutation::PolicyNoPendingCap) {
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
                reason: String::from(reason),
                hash,
            },
        );
        self.audit.push(Audit::Submitted { id, account });
        Ok(id)
    }

    /// What `approver` sees on its approval screen: its own principal's requests, and a labelled
    /// one only if it owns every label on it. Rendered by the steward from the structured request;
    /// the requester's reason is quoted, escaped, capped and marked untrusted.
    pub fn pending(&self, approver: usize) -> Vec<Rendered> {
        let owned = &self.principals[approver].spec.owned_labels;
        let mut out = Vec::new();
        for r in self.requests.values() {
            let visible = r.approver == approver && r.labels.iter().all(|l| owned.contains(l));
            if !visible && !self.broken(Mutation::PolicyShowLabelledToAll) {
                continue;
            }
            let who = sanitize(&self.principals[r.approver].spec.name, FIELD_CAP);
            let what = match &r.content {
                Content::AgentWithLabel { label, lease } => {
                    format!("start an agent labelled {label} for {lease} us")
                }
                Content::Declassify { label, item } => {
                    let shown =
                        r.snapshot.as_ref().map(|b| sanitize(&String::from_utf8_lossy(b), DECLASSIFY_MAX));
                    format!("declassify item {item} of label {label}: \"{}\"", shown.unwrap_or_default())
                }
                Content::Note { what } => sanitize(what, FIELD_CAP),
            };
            let text = format!(
                "request from {who} (session {}): {what}; reason (untrusted): \"{}\"",
                r.session,
                sanitize(&r.reason, FIELD_CAP)
            );
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
        self.reconcile();
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
        self.requests.remove(&id);
        self.audit.push(Audit::Approved { id, principal: ch.principal, key: ch.key, hash });
        match r.content {
            Content::AgentWithLabel { label, lease } => {
                let deadline = self.k.now.saturating_add(lease.max(1));
                let sid =
                    self.new_session(ch.principal, SessionKind::Agent, vec![label], AGENT_PAGES, deadline)?;
                self.audit.push(Audit::AgentStarted {
                    session: sid,
                    sponsor: ch.principal,
                    labels: vec![label],
                    deadline,
                });
            }
            Content::Declassify { label, item } => {
                let bytes = match r.snapshot {
                    Some(s) => s,
                    None => self.vault.get(&(label, item)).cloned().unwrap_or_default(),
                };
                self.declassified.push((label, bytes.clone()));
                self.audit.push(Audit::Declassified { id, label, bytes });
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
        self.requests.remove(&id);
        self.audit.push(Audit::Denied { id, principal: ch.principal });
        Ok(())
    }

    // -------------------------------------------------------------------------------------------
    // Crash blame (CONTAINMENT.md): init passes each exit notice's blamed account on.

    /// Three crashes blamed on one account within ten minutes log that account out: every
    /// session and agent of it is destroyed. Account 0 (nothing being served) blames nobody.
    pub fn blame(&mut self, account: u64) {
        self.reconcile();
        if account == 0 {
            return;
        }
        let now = self.k.now;
        self.audit.push(Audit::Blamed { account, at: now });
        let window = !self.broken(Mutation::PolicyBlameNoWindow);
        let times = self.blames.entry(account).or_default();
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
                .map(|s| s.id)
                .collect();
            for s in doomed {
                let _ = self.end_session(s);
            }
            self.audit.push(Audit::LoggedOut { account, at: now });
        }
    }

    pub fn keyd_add(&mut self, key: u64) {
        self.keyd.insert(key);
    }
}
