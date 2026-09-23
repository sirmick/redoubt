//! Property tests for the steward's policy (steward.rs): random policy operations, with the
//! policy's properties and the kernel's invariants checked after each one.
//!
//! | Property | Statement | Source |
//! | --- | --- | --- |
//! | P1 sessions | a session's budget is carved from its principal's fixed sub-budget for its label set (a sub-agent's from its agent's), with its account; labels are none or one the principal owns | CONTAINMENT.md, "Sessions and vaults"; QUESTIONS 89 |
//! | P2 login | a login used one of the principal's login keys, never one `keyd` holds | CAPABILITIES.md, "The powerbox and approvals" |
//! | P3 approvals | an approval came through `approve@box` with the approver's approval key, named the frozen content's hash, and granted no label the approver lacks | CAPABILITIES.md, "Binding", "Limits and labels" |
//! | P4 screens | an approver sees only its own requests, labelled ones only if it owns every label; rendered text is printable ASCII with capped free text; a labelled request shows none of its free text | CAPABILITIES.md, "Rendering"; QUESTIONS 34, 35 |
//! | P5 cap | at most `PENDING_CAP` pending requests per (account, label set), all of live sessions; a session holds at most its fair share | CONTAINMENT.md, "The shared server library"; QUESTIONS 17, 90 |
//! | P6 declassification | what is copied out is exactly the snapshot taken at submission, read through a reader budget carrying exactly the item's label | CONTAINMENT.md, "Declassification"; QUESTIONS 54 |
//! | P7 blame | an (account, label set)'s sessions are logged out exactly when three server crashes blamed on it (by the kernel's exit notices) fall within ten minutes; no other sessions are touched; no session of it starts for the next ten minutes | CONTAINMENT.md, "Crash blame"; INIT.md; QUESTIONS 48, 91 |
//! | P8 labelled sessions | a labelled session starts nothing; it only submits requests | CONTAINMENT.md, "The shared server library" (steward) |
//! | P9 leases | an agent's budget has a deadline at most `MAX_LEASE` away; a sub-agent sits in its agent's budget and ends no later; an expired lease is gone | CAPABILITIES.md, "Agents"; QUESTIONS 33 |
//! | P10 non-interference | a vault session's work (item writes, requests, calls to a shared server) changes nothing an unlabelled session observes: its results, the usage of `users`, of every principal's budget and unlabelled sub-budget, and the audit records an unlabelled reader may read | PLAN.md, attack suite "no leaky state"; CONTAINMENT.md; QUESTIONS 89, 92 |
//! | P11 writes | every write to an item is by a session with exactly the item's labels | CONTAINMENT.md, `check`; QUESTIONS 51 |
//! | P12 system budgets | only `init` and the steward hold a handle to a system-class budget; a session's connection to the server is narrowed to a revocation scope inside its session | QUESTIONS 79, 80 |
//! | P13 leases end | a lease's sponsor can always end it | CAPABILITIES.md; QUESTIONS 90 |

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::check::Failure;
use crate::gen::Rng;
use crate::invariants::Checker;
use crate::mutation::Mutation;
use crate::spec::SLICE;
use crate::steward::*;

/// A request, named by who submitted it and when, so a sequence means the same thing when
/// replayed with some operations removed (P10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReqRef {
    pub session: u64,
    pub nth: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashRef {
    /// The request's own hash.
    Own,
    /// Another request's hash (a swapped approval).
    Of(ReqRef),
    Literal(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyOp {
    Login {
        principal: usize,
        label: Option<u64>,
        key: u64,
    },
    EndSession {
        session: u64,
    },
    StartAgent {
        session: u64,
        lease: u64,
    },
    WriteItem {
        session: u64,
        label: u64,
        item: u64,
        bytes: Vec<u8>,
    },
    Submit {
        session: u64,
        content: Content,
        reason: String,
    },
    Pending {
        principal: usize,
    },
    Approve {
        principal: usize,
        key: u64,
        request: ReqRef,
        hash: HashRef,
    },
    Deny {
        principal: usize,
        key: u64,
        request: ReqRef,
    },
    KeydAdd {
        key: u64,
    },
    Usage {
        principal: usize,
    },
    /// The session's process calls the server.
    Work {
        session: u64,
    },
    /// Session `by` ends agent session `lease`.
    EndLease {
        by: u64,
        lease: u64,
    },
    /// The server answers everything waiting.
    Serve,
    /// The server takes one message and keeps it open.
    Hold,
    /// The server crashes.
    Crash,
    /// A session calls the server, which takes the call and crashes serving it.
    CrashServing {
        session: u64,
    },
    Tick {
        dt: u64,
    },
}

/// The test manifest: alice owns labels 7 and 8, bob owns 9, carol none. Keys: login 11/21/31,
/// approval 12/22/32; keyd holds 100 and 101.
pub fn manifest() -> Manifest {
    let p = |name: &str, account, login, approval, labels: &[u64]| PrincipalSpec {
        name: String::from(name),
        account,
        login_keys: vec![login],
        approval_keys: vec![approval],
        owned_labels: labels.to_vec(),
        pages: 500,
        weight: 100,
    };
    Manifest {
        principals: vec![
            p("alice", 1001, 11, 12, &[7, 8]),
            p("bob", 1002, 21, 22, &[9]),
            p("carol", 1003, 31, 32, &[]),
        ],
        keyd_keys: vec![100, 101],
    }
}

const ALL_KEYS: [u64; 8] = [11, 12, 21, 22, 31, 32, 100, 101];

fn text(rng: &mut Rng, max: u64) -> String {
    let n = rng.below(max + 1);
    (0..n)
        .map(|_| match rng.below(24) {
            0 => '\u{1b}',
            1 => '\n',
            2 => '"',
            3 => '\\',
            4 => '\u{7}',
            5 => '\u{202e}', // right-to-left override
            6 => '\u{2066}', // left-to-right isolate
            7 => '\u{200b}', // zero-width space
            _ => (b'a' + rng.below(26) as u8) as char,
        })
        .collect()
}

/// A lease: usually short, now and then hostile (over `MAX_LEASE`, `u64::MAX`, 0).
fn lease(rng: &mut Rng) -> u64 {
    match rng.below(12) {
        0 => u64::MAX,
        1 => MAX_LEASE + 1,
        2 => 0,
        3 => MAX_LEASE,
        _ => rng.range(1, 400) * SLICE,
    }
}

/// A random policy operation, biased toward what exists.
pub fn random_op(st: &Steward, subs: &[ReqRef], rng: &mut Rng) -> PolicyOp {
    let sessions: Vec<u64> = st.sessions.keys().copied().collect();
    let session = |rng: &mut Rng| rng.pick(&sessions).unwrap_or(rng.range(1, 20));
    let principal = rng.below(st.principals.len() as u64) as usize;
    let request = |rng: &mut Rng| rng.pick(subs).unwrap_or(ReqRef { session: 1, nth: 0 });
    match rng.below(100) {
        0..=11 => {
            let owned = st.principals[principal].spec.owned_labels.clone();
            let label = match rng.below(10) {
                0..=4 => None,
                5..=8 => rng.pick(&owned).or(Some(9)),
                _ => Some(rng.range(7, 9)),
            };
            let good = st.principals[principal].spec.login_keys[0];
            let key = if rng.pct(80) { good } else { rng.pick(&ALL_KEYS).unwrap() };
            PolicyOp::Login { principal, label, key }
        }
        12..=14 => PolicyOp::EndSession { session: session(rng) },
        15..=21 => PolicyOp::StartAgent { session: session(rng), lease: lease(rng) },
        22..=28 => {
            let len = if rng.pct(85) { rng.below(40) } else { rng.range(200, 300) };
            let bytes = (0..len).map(|_| if rng.pct(97) { b'a' + rng.below(26) as u8 } else { 7 }).collect();
            PolicyOp::WriteItem { session: session(rng), label: rng.range(7, 9), item: rng.below(3), bytes }
        }
        29..=43 => {
            let content = match rng.below(3) {
                0 => Content::Note { what: text(rng, 90) },
                1 => Content::AgentWithLabel { label: rng.range(7, 9), lease: lease(rng) },
                _ => Content::Declassify { label: rng.range(7, 9), item: rng.below(3) },
            };
            PolicyOp::Submit { session: session(rng), content, reason: text(rng, 100) }
        }
        44..=48 => PolicyOp::Pending { principal },
        49..=58 => {
            let good = st.principals[principal].spec.approval_keys[0];
            let key = if rng.pct(85) { good } else { rng.pick(&ALL_KEYS).unwrap() };
            let hash = match rng.below(10) {
                0 => HashRef::Of(request(rng)),
                1 => HashRef::Literal(rng.next_u64()),
                _ => HashRef::Own,
            };
            PolicyOp::Approve { principal, key, request: request(rng), hash }
        }
        59..=60 => {
            let key = st.principals[principal].spec.approval_keys[0];
            PolicyOp::Deny { principal, key, request: request(rng) }
        }
        61..=62 => PolicyOp::KeydAdd { key: rng.pick(&ALL_KEYS).unwrap() },
        63..=64 => PolicyOp::Usage { principal },
        65..=76 => PolicyOp::Work { session: session(rng) },
        77..=79 => PolicyOp::Serve,
        80..=81 => PolicyOp::Hold,
        82..=83 => PolicyOp::Crash,
        84..=88 => PolicyOp::CrashServing { session: session(rng) },
        89..=91 => {
            // Usually an agent and a session of its principal.
            let agents: Vec<u64> =
                st.sessions.values().filter(|s| s.kind == SessionKind::Agent).map(|s| s.id).collect();
            let lease = rng.pick(&agents).unwrap_or_else(|| session(rng));
            let p = st.sessions.get(&lease).map(|s| s.principal);
            let mine: Vec<u64> = st
                .sessions
                .values()
                .filter(|s| Some(s.principal) == p && s.labels.is_empty())
                .map(|s| s.id)
                .collect();
            let by = if rng.pct(80) { rng.pick(&mine).unwrap_or_else(|| session(rng)) } else { session(rng) };
            PolicyOp::EndLease { by, lease }
        }
        _ => PolicyOp::Tick {
            // Minutes half the time, so that crash blame's ten-minute window is crossed.
            dt: match rng.below(4) {
                0 => rng.range(1, 50) * SLICE,
                1 | 2 => rng.range(1, 6) * 60_000_000,
                _ => rng.range(1, 400) * SLICE,
            },
        },
    }
}

/// What an operation showed its caller, for P10.
pub type Obs = String;

/// A run of policy operations with its own record of what was submitted and blamed, independent
/// of the steward's.
pub struct Run {
    pub st: Steward,
    checker: Checker,
    /// (session, nth) -> request id, as the steward returned it.
    pub submitted: BTreeMap<(u64, u64), u64>,
    pub subs: Vec<ReqRef>,
    /// Request id -> (hash from the steward's screen at submission, declassified bytes then).
    pub ghost_requests: BTreeMap<u64, (u64, Option<Vec<u8>>)>,
    /// Blame times per (account, label set).
    pub ghost_blames: BTreeMap<(u64, Vec<u64>), Vec<u64>>,
    /// The account and labels of the call the server works on (from what `hold` returned), if any.
    held: Option<(u64, Vec<u64>)>,
    /// (account, label set)s logged out, and when their lockout ends (P7; QUESTIONS 91).
    pub ghost_locked: BTreeMap<(u64, Vec<u64>), u64>,
    pub per_session: BTreeMap<u64, u64>,
}

impl Run {
    pub fn new(mutation: Option<Mutation>, secret: u64) -> Run {
        let st = Steward::new(&manifest(), secret, mutation).expect("the test manifest is valid");
        let checker = Checker::new(&st.k);
        Run {
            st,
            checker,
            submitted: BTreeMap::new(),
            subs: Vec::new(),
            ghost_requests: BTreeMap::new(),
            ghost_blames: BTreeMap::new(),
            held: None,
            ghost_locked: BTreeMap::new(),
            per_session: BTreeMap::new(),
        }
    }

    fn resolve(&self, r: ReqRef) -> u64 {
        self.submitted
            .get(&(r.session, r.nth))
            .copied()
            .unwrap_or(r.session.wrapping_mul(7919).wrapping_add(r.nth))
    }

    /// Apply one op, check P1-P9 and the kernel invariants, and return what the caller saw.
    pub fn apply(&mut self, op: &PolicyOp) -> Result<Obs, String> {
        let audit_from = self.st.audit.len();
        let sessions_before: BTreeMap<u64, Session> = self.st.sessions.clone();
        let now = self.st.k.now;
        let obs = match op {
            PolicyOp::Login { principal, label, key } => {
                let name = self.st.principals[*principal].spec.name.clone();
                format!("{:?}", self.st.login(&name, *label, *key))
            }
            PolicyOp::EndSession { session } => format!("{:?}", self.st.end_session(*session)),
            PolicyOp::StartAgent { session, lease } => {
                let requester = self.st.sessions.get(session).cloned();
                let r = self.st.start_agent(*session, *lease);
                if let (Some(req), Ok(id)) = (&requester, &r) {
                    if !req.labels.is_empty() {
                        return Err(format!("P8: labelled session {session} started an agent"));
                    }
                    let new = &self.st.sessions[id];
                    let b = &self.st.k.budgets[&new.budget];
                    if req.kind == SessionKind::Agent {
                        let under = self.st.k.is_descendant_or_self(new.budget, req.budget);
                        let ends = b.deadline.is_some_and(|d| req.deadline.is_some_and(|rd| d <= rd));
                        if !under || !ends {
                            return Err(format!(
                                "P9: agent {session}'s sub-agent is outside it or outlives it"
                            ));
                        }
                    }
                }
                format!("{r:?}")
            }
            PolicyOp::WriteItem { session, label, item, bytes } => {
                format!("{:?}", self.st.write_item(*session, *label, *item, bytes.clone()))
            }
            PolicyOp::Submit { session, content, reason } => {
                // What a snapshot must contain, read from the vault before the steward acts.
                let expect = match content {
                    Content::Declassify { label, item } => {
                        Some(self.st.vault.get(&(*label, *item)).cloned().unwrap_or_default())
                    }
                    _ => None,
                };
                let r = self.st.submit(*session, content.clone(), reason);
                if r.is_ok() && self.st.pending_by(*session) > self.st.share(*session) {
                    return Err(format!(
                        "P5: session {session} holds {} pending requests, over its fair share {}",
                        self.st.pending_by(*session),
                        self.st.share(*session)
                    ));
                }
                if let Ok(id) = r {
                    let nth = *self.per_session.entry(*session).or_default();
                    self.per_session.insert(*session, nth + 1);
                    self.submitted.insert((*session, nth), id);
                    self.subs.push(ReqRef { session: *session, nth });
                    let hash = self.st.requests[&id].hash;
                    self.ghost_requests.insert(id, (hash, expect));
                    // An id is shown only to its submitter: record that it is fresh, not its value.
                    format!("Ok(request {nth})")
                } else {
                    format!("{r:?}")
                }
            }
            PolicyOp::Pending { principal } => {
                let screen = self.st.pending(*principal);
                let owned = &self.st.principals[*principal].spec.owned_labels;
                for r in &screen {
                    let req = &self.st.requests[&r.id];
                    let visible = req.approver == *principal && req.labels.iter().all(|l| owned.contains(l));
                    if !visible {
                        return Err(format!(
                            "P4: principal {principal} sees request {} labelled {:?}",
                            r.id, r.labels
                        ));
                    }
                    if r.text.chars().any(|c| !(' '..='~').contains(&c)) {
                        return Err(format!(
                            "P4: rendered request {} is not printable ASCII: {:?}",
                            r.id, r.text
                        ));
                    }
                    // A labelled request shows only text the steward generates (QUESTIONS 35).
                    let note = match &req.content {
                        Content::Note { what } => what.clone(),
                        _ => String::new(),
                    };
                    for free in [&req.reason, &note] {
                        let shown = sanitize(free, FIELD_CAP, true);
                        if !req.labels.is_empty()
                            && !shown.is_empty()
                            && r.text.contains(&format!("\"{shown}\""))
                        {
                            return Err(format!(
                                "P4: labelled request {} shows its free text: {:?}",
                                r.id, r.text
                            ));
                        }
                    }
                }
                format!("{} requests", screen.len())
            }
            PolicyOp::Approve { principal, key, request, hash } => {
                let name = self.st.principals[*principal].spec.name.clone();
                let id = self.resolve(*request);
                let h = match hash {
                    HashRef::Own => self.st.requests.get(&id).map_or(0, |r| r.hash),
                    HashRef::Of(other) => self.st.requests.get(&self.resolve(*other)).map_or(1, |r| r.hash),
                    HashRef::Literal(x) => *x,
                };
                let r = self.st.open_approval(&name, *key).and_then(|ch| self.st.approve(ch, id, h));
                format!("{r:?}")
            }
            PolicyOp::Deny { principal, key, request } => {
                let name = self.st.principals[*principal].spec.name.clone();
                let id = self.resolve(*request);
                let r = self.st.open_approval(&name, *key).and_then(|ch| self.st.deny(ch, id));
                format!("{r:?}")
            }
            PolicyOp::KeydAdd { key } => {
                self.st.keyd_add(*key);
                String::from("ok")
            }
            PolicyOp::Usage { principal } => {
                let h = self.st.principals[*principal].h;
                format!("{:?}", self.st.usage(h))
            }
            PolicyOp::Work { session } => format!("{:?}", self.st.work(*session)),
            PolicyOp::EndLease { by, lease } => {
                let sponsor = match (self.st.sessions.get(by), self.st.sessions.get(lease)) {
                    (Some(b), Some(l)) => {
                        b.labels.is_empty() && l.kind == SessionKind::Agent && b.principal == l.principal
                    }
                    _ => false,
                };
                let r = self.st.end_lease(*by, *lease);
                if sponsor && (r.is_err() || self.st.sessions.contains_key(lease)) {
                    return Err(format!("P13: session {by} could not end its agent {lease}: {r:?}"));
                }
                format!("{r:?}")
            }
            PolicyOp::Serve => {
                self.st.serve();
                self.held = None;
                String::from("ok")
            }
            PolicyOp::Hold => {
                self.held = self.st.hold().map(|m| (m.account, m.labels));
                String::from("ok")
            }
            PolicyOp::Crash => {
                let held = self.held.take().unwrap_or_default();
                self.st.crash_server();
                self.check_blame(held, now, audit_from, &sessions_before)?;
                String::from("ok")
            }
            PolicyOp::CrashServing { session } => {
                let r = self.st.work(*session);
                self.st.poll();
                self.held = self.st.hold().map(|m| (m.account, m.labels));
                let held = self.held.take().unwrap_or_default();
                self.st.crash_server();
                self.check_blame(held, now, audit_from, &sessions_before)?;
                format!("{r:?}")
            }
            PolicyOp::Tick { dt } => {
                self.st.tick(*dt);
                String::from("ok")
            }
        };
        self.st.poll();
        // P7: no session of a logged-out (account, label set) starts within its window.
        for s in self.st.sessions.values() {
            let key = (self.st.principals[s.principal].spec.account, s.labels.clone());
            let locked = self.ghost_locked.get(&key).is_some_and(|until| now < *until);
            if locked && !sessions_before.contains_key(&s.id) {
                return Err(format!("P7: session {} of {key:?} started while it was locked out", s.id));
            }
        }
        self.check(audit_from)?;
        Ok(obs)
    }

    /// P7: the crash was blamed on `held` (the account and labels of the newest message the server
    /// held), and sessions of that account and label set were logged out exactly when three such
    /// blames fall within ten minutes.
    fn check_blame(
        &mut self,
        held: (u64, Vec<u64>),
        now: u64,
        audit_from: usize,
        before: &BTreeMap<u64, Session>,
    ) -> Result<(), String> {
        let (account, labels) = held;
        let blamed: Vec<(u64, Vec<u64>)> = self.st.audit[audit_from..]
            .iter()
            .filter_map(|a| match a {
                Audit::Blamed { account, labels, .. } => Some((*account, labels.clone())),
                _ => None,
            })
            .collect();
        let key = (account, labels.clone());
        if account != 0 && !blamed.contains(&key) {
            return Err(format!("P7: the server crashed serving {key:?}, which was not blamed"));
        }
        if account == 0 && !blamed.is_empty() {
            return Err(format!("P7: a crash serving nobody blamed {blamed:?}"));
        }
        if account == 0 {
            return Ok(());
        }
        let times = self.ghost_blames.entry(key).or_default();
        times.push(now);
        times.retain(|t| now - *t < BLAME_WINDOW);
        let logout = times.len() >= BLAME_COUNT;
        if logout {
            times.clear();
            self.ghost_locked.insert((account, labels.clone()), now + BLAME_WINDOW);
        }
        let principal = self.st.principals.iter().position(|p| p.spec.account == account);
        for (id, s) in before {
            let mine = Some(s.principal) == principal && s.labels == labels;
            // Gone, and not by its lease running out: ended by the steward.
            let still = self.st.sessions.contains_key(id);
            let ended = !still && s.deadline.is_none_or(|d| d > self.st.k.now);
            if mine && logout && still {
                return Err(format!(
                    "P7: account {account} with labels {labels:?} blamed 3 times in 10 minutes, session {id} survived"
                ));
            }
            if (!mine || !logout) && ended {
                return Err(format!(
                    "P7: blaming account {account} ended session {id} of principal {}",
                    s.principal
                ));
            }
        }
        Ok(())
    }

    /// P1-P6 and P9 on the state, and on the audit entries this step added; the kernel's
    /// invariants on the kernel underneath.
    fn check(&mut self, audit_from: usize) -> Result<(), String> {
        if !self.st.audit_authentic() {
            return Err(String::from("P14: altered or unsigned audit record"));
        }
        let st = &self.st;
        self.checker.check(&st.k)?;
        for s in st.sessions.values() {
            let p = &st.principals[s.principal];
            let b = st.k.budgets.get(&s.budget).ok_or(format!("P1: session {} has no budget", s.id))?;
            let under = st.k.is_descendant_or_self(s.budget, p.budget) && s.budget != p.budget;
            if !under || b.account != p.spec.account || b.labels != s.labels {
                return Err(format!("P1: session {}'s budget is not its principal's", s.id));
            }
            // Carved from the principal's sub-budget for its label set, or (a sub-agent) from an
            // agent's budget of the same principal.
            let sub = p.subs.get(&s.labels).map(|x| x.0);
            let in_agent = st.sessions.values().any(|a| {
                a.kind == SessionKind::Agent && a.principal == s.principal && Some(a.budget) == b.parent
            });
            if b.parent != sub && !(s.kind == SessionKind::Agent && in_agent) {
                return Err(format!(
                    "P1: session {} with labels {:?} is not carved from its principal's sub-budget for them",
                    s.id, s.labels
                ));
            }
            if s.labels.len() > 1 || !s.labels.iter().all(|l| p.spec.owned_labels.contains(l)) {
                return Err(format!(
                    "P1: session {} carries labels {:?} its principal does not own",
                    s.id, s.labels
                ));
            }
            if s.kind == SessionKind::Agent && b.deadline.is_none_or(|d| d <= st.k.now) {
                return Err(format!("P9: agent session {} has no future deadline", s.id));
            }
        }
        // P12: only init and the steward hold system budgets; connections are narrowed to scopes.
        for p in st.k.processes.values() {
            if p.pid == crate::kernel::INIT_PID || p.pid == st.me.pid {
                continue;
            }
            for h in p.handles.values() {
                if let crate::kernel::Object::Budget(b) = h.object {
                    if st.k.budgets.get(&b).is_some_and(|x| x.class == crate::spec::Class::System) {
                        return Err(format!("P12: process {} holds system-class budget {b}", p.pid));
                    }
                }
            }
        }
        for s in st.sessions.values() {
            let Some(p) = st.k.processes.get(&s.pid) else { continue };
            for h in p.handles.values().filter(|h| matches!(h.object, crate::kernel::Object::Endpoint(_))) {
                let stamp = st.k.budgets.get(&h.stamp);
                let scope =
                    stamp.is_some_and(|x| x.pages_limit == 0 && x.processes_limit == 0 && x.weight == 0);
                if !scope || !st.k.is_descendant_or_self(h.stamp, s.budget) {
                    return Err(format!(
                        "P12: session {}'s connection is stamped {}, not a scope in its session",
                        s.id, h.stamp
                    ));
                }
            }
        }
        let mut pending: BTreeMap<(u64, Vec<u64>), usize> = BTreeMap::new();
        for r in st.requests.values() {
            *pending.entry((r.account, r.labels.clone())).or_default() += 1;
            if !st.sessions.contains_key(&r.session) {
                return Err(format!("P5: request {} of ended session {} is still pending", r.id, r.session));
            }
        }
        if let Some((a, n)) = pending.iter().find(|(_, n)| **n > PENDING_CAP) {
            return Err(format!("P5: {a:?} has {n} pending requests"));
        }
        for e in &st.audit[audit_from..] {
            match e {
                Audit::Login { principal, key, .. }
                    if !st.principals[*principal].spec.login_keys.contains(key) || st.keyd.contains(key) =>
                {
                    return Err(format!("P2: login to principal {principal} with key {key}"));
                }
                Audit::Approved { id, principal, key, hash, .. } => {
                    let spec = &st.principals[*principal].spec;
                    let Some((want, _)) = self.ghost_requests.get(id) else {
                        return Err(format!("P3: approved request {id} was never submitted"));
                    };
                    if hash != want {
                        return Err(format!(
                            "P3: request {id} approved with hash {hash:#x}, its content hashes to {want:#x}"
                        ));
                    }
                    if !spec.approval_keys.contains(key)
                        || spec.login_keys.contains(key)
                        || st.keyd.contains(key)
                    {
                        return Err(format!("P3: request {id} approved with key {key}, not an approval key"));
                    }
                }
                Audit::AgentStarted { sponsor, labels, deadline, .. } => {
                    if !labels.iter().all(|l| st.principals[*sponsor].spec.owned_labels.contains(l)) {
                        return Err(format!(
                            "P3: an agent labelled {labels:?} started for a principal owning fewer"
                        ));
                    }
                    if *deadline > st.k.now.saturating_add(MAX_LEASE) {
                        return Err(format!(
                            "P9: an agent's lease ends at {deadline}, over MAX_LEASE from {}",
                            st.k.now
                        ));
                    }
                }
                Audit::Declassified { id, bytes, label, reader } => {
                    let want = self.ghost_requests.get(id).and_then(|(_, b)| b.clone());
                    if want.as_ref() != Some(bytes) {
                        return Err(format!(
                            "P6: declassified {bytes:?}, the snapshot at submission was {want:?}"
                        ));
                    }
                    // The kernel's own record of the reader budget (QUESTIONS 54).
                    let labels = st.k.ghost.labels_at_creation.get(reader);
                    if labels != Some(&vec![*label]) {
                        return Err(format!(
                            "P6: item of label {label} read through budget {reader} with labels {labels:?}"
                        ));
                    }
                }
                Audit::Wrote { session, labels, label } if labels != &vec![*label] => {
                    return Err(format!(
                        "P11: session {session} with labels {labels:?} wrote an item of {label}"
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// P1-P9: a random sequence of policy operations.
pub fn steward_policy(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    let mut rng = Rng::new(seed);
    let mut run = Run::new(mutation, rng.next_u64());
    for i in 0..rng.range(20, 160) {
        let op = random_op(&run.st, &run.subs, &mut rng);
        run.apply(&op).map_err(|message| Failure {
            family: "steward_policy",
            seed,
            message: format!("{message} (at op {i}: {op:?})"),
            ops: Vec::new(),
        })?;
    }
    Ok(())
}

/// P10 (PLAN.md's "no leaky state" attack case; CONTAINMENT.md): one sequence runs twice, the
/// second time without the work of the vault sessions (their item writes, submissions and calls to
/// the shared server). Everything an unlabelled session observes must be the same: every result it
/// gets, and the usage of every principal's top budget and of `users`.
///
/// Since the caps are keyed by (account, label set) (QUESTIONS 17), the observers include the
/// vault owner's own unlabelled sessions. Left out of the sequence: approving or denying a vault
/// request (approving is declassifying, by design), ending a vault session, and the server's
/// crashes (crash blame is per account, so a vault crashing the server could log out its owner's
/// unlabelled sessions: README, open questions). The owner's approval screen is not an observer.
pub fn steward_noninterference(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    let mut rng = Rng::new(seed);
    let secret = rng.next_u64();
    let fail =
        |message: String| Failure { family: "steward_noninterference", seed, message, ops: Vec::new() };
    // Build the sequence on a first run, recording which ops are vault work.
    let mut first = Run::new(mutation, secret);
    let mut ops: Vec<(PolicyOp, bool)> = Vec::new();
    let mut vault_sessions: BTreeSet<u64> = BTreeSet::new();
    let mut owners: BTreeSet<usize> = BTreeSet::new();
    for _ in 0..rng.range(20, 100) {
        let op = random_op(&first.st, &first.subs, &mut rng);
        let vault = match &op {
            PolicyOp::WriteItem { session, .. }
            | PolicyOp::Submit { session, .. }
            | PolicyOp::Work { session } => vault_sessions.contains(session),
            PolicyOp::Approve { request, .. } | PolicyOp::Deny { request, .. } => {
                if vault_sessions.contains(&request.session) {
                    continue;
                }
                false
            }
            PolicyOp::EndSession { session } if vault_sessions.contains(session) => continue,
            PolicyOp::Hold | PolicyOp::Crash | PolicyOp::CrashServing { .. } => continue,
            _ => false,
        };
        let before = first.st.sessions.keys().copied().collect::<BTreeSet<u64>>();
        first.apply(&op).map_err(fail)?;
        for s in first.st.sessions.values() {
            if !before.contains(&s.id) && !s.labels.is_empty() {
                vault_sessions.insert(s.id);
                owners.insert(s.principal);
            }
        }
        ops.push((op, vault));
    }
    let mut with = Run::new(mutation, secret);
    let mut without = Run::new(mutation, secret);
    for (i, (op, vault)) in ops.iter().enumerate() {
        let observed = match op {
            PolicyOp::Pending { principal } => !owners.contains(principal),
            PolicyOp::EndSession { session }
            | PolicyOp::StartAgent { session, .. }
            | PolicyOp::WriteItem { session, .. }
            | PolicyOp::Submit { session, .. }
            | PolicyOp::Work { session } => {
                with.st.sessions.get(session).is_some_and(|s| s.labels.is_empty())
            }
            PolicyOp::Login { label, .. } => label.is_none(),
            PolicyOp::EndLease { by, .. } => with.st.sessions.get(by).is_some_and(|s| s.labels.is_empty()),
            PolicyOp::Approve { .. } | PolicyOp::Deny { .. } | PolicyOp::Usage { .. } => true,
            _ => false,
        };
        let a = with.apply(op).map_err(fail)?;
        if *vault {
            continue;
        }
        let b = without.apply(op).map_err(fail)?;
        if observed && a != b {
            return Err(fail(format!(
                "P10: op {i} ({op:?}) showed {a:?} with the vault's work and {b:?} without"
            )));
        }
        // Ids of unlabelled sessions' requests.
        for (key, id) in &with.submitted {
            let unlabelled = with.st.sessions.get(&key.0).is_some_and(|s| s.labels.is_empty());
            if unlabelled && without.submitted.get(key) != Some(id) {
                return Err(fail(format!(
                    "P10: session {}'s request {} got a different id without the vault's work",
                    key.0, key.1
                )));
            }
        }
        // What an unlabelled reader may read of the audit file (QUESTIONS 92).
        let (x, y) = (format!("{:?}", with.st.audit_view(&[])), format!("{:?}", without.st.audit_view(&[])));
        if x != y {
            return Err(fail(format!("P10: the unlabelled audit view depends on the vault's work (op {i})")));
        }
        // The steward's slot 1 is `users`; each principal's top budget and unlabelled sub-budget.
        let shared: Vec<u64> = core::iter::once(1)
            .chain(with.st.principals.iter().map(|p| p.h))
            .chain(with.st.principals.iter().filter_map(|p| p.subs.get(&Vec::new()).map(|s| s.1)))
            .collect();
        for h in shared {
            let (x, y) = (with.st.usage(h), without.st.usage(h));
            if x != y {
                return Err(fail(format!(
                    "P10: usage {x:?} depends on the vault's work ({y:?} without; op {i})"
                )));
            }
        }
    }
    Ok(())
}

/// Independent ancestry oracle for answer 117. The model stores a root at grant time; this
/// checker instead walks immutable parent edges and reconstructs connected shares after every
/// operation, including after disconnect. `break_lineage` deliberately assigns self-minted
/// descendants new roots to establish that the property can fail.
pub fn connection_lineage(seed: u64, steps: usize, break_lineage: bool) -> Result<(), String> {
    #[derive(Clone)]
    struct Grant {
        parent: Option<u64>,
        account: u64,
        labels: Vec<u64>,
        live: bool,
        used: usize,
    }
    fn same(a: &Grant, b: &Grant, a_id: u64, b_id: u64) -> bool {
        a.account == b.account && a.labels == b.labels && (a.account != 0 || a_id == b_id)
    }
    fn ancestor(grants: &BTreeMap<u64, Grant>, mut id: u64) -> u64 {
        while let Some(parent) = grants[&id].parent {
            if !same(&grants[&id], &grants[&parent], id, parent) {
                break;
            }
            id = parent;
        }
        id
    }
    let mut rng = Rng::new(seed);
    let mut model = ConnectionShares::new(8).unwrap();
    let mut grants: BTreeMap<u64, Grant> = BTreeMap::new();
    let mut next = 1;
    for step in 0..steps {
        let live: Vec<_> = grants.iter().filter(|(_, g)| g.live).map(|(id, _)| *id).collect();
        let id = rng.pick(&live);
        match (rng.below(10), id) {
            (0..=3, _) | (_, None) => {
                let parent = id.filter(|_| rng.pct(75));
                let (account, labels) = if parent.is_some() && rng.pct(80) {
                    let p = &grants[&parent.unwrap()];
                    (p.account, p.labels.clone())
                } else {
                    (rng.below(3), if rng.pct(50) { vec![] } else { vec![7] })
                };
                model.grant(next, account, labels.clone(), parent).map_err(|e| format!("grant: {e:?}"))?;
                grants.insert(next, Grant { parent, account, labels, live: true, used: 0 });
                if break_lineage {
                    model.connections.get_mut(&next).unwrap().root = next;
                }
                next += 1;
            }
            (4..=7, Some(id)) => {
                let g = &grants[&id];
                let bucket: Vec<_> =
                    grants.iter().filter(|(other, x)| x.live && same(g, x, id, **other)).collect();
                let roots: BTreeSet<_> = bucket.iter().map(|(other, _)| ancestor(&grants, **other)).collect();
                let used: usize = bucket.iter().map(|(_, x)| x.used).sum();
                let root = ancestor(&grants, id);
                let share: usize = bucket
                    .iter()
                    .filter(|(other, _)| ancestor(&grants, **other) == root)
                    .map(|(_, x)| x.used)
                    .sum();
                let allowed = used < 8 && share < (8 / roots.len()).max(1);
                let result = model.admit(id);
                if result.is_ok() != allowed {
                    return Err(format!("P15: admission mismatch at step {step}, badge {id}"));
                }
                if allowed {
                    grants.get_mut(&id).unwrap().used += 1;
                }
            }
            (8, Some(id)) => {
                let used = grants[&id].used;
                if model.release(id).is_ok() != (used > 0) {
                    return Err(format!("P15: release at {step}"));
                }
                if used > 0 {
                    grants.get_mut(&id).unwrap().used -= 1;
                }
            }
            (_, Some(id)) => {
                model.disconnect(id).unwrap();
                grants.get_mut(&id).unwrap().live = false;
            }
        }
        for (id, c) in &model.connections {
            if c.root != ancestor(&grants, *id) || c.used != grants[id].used {
                return Err(format!("P15: lineage or usage mismatch at step {step}, badge {id}"));
            }
        }
    }
    Ok(())
}

/// A specification-side observable read oracle. Confined read-down must fail even when the
/// lower volume exists and ordinary subset-based `check` would allow the read. Accepting a
/// supplied success here is the deliberate rule-break control in `policy_current`.
pub fn confined_read_observation(
    confined: bool,
    caller_labels: &[u64],
    object_label: Option<u64>,
    result: &Result<Vec<u8>, Denied>,
) -> Result<(), String> {
    if confined && !caller_labels.is_empty() && object_label.is_none() && result != &Err(Denied::ReadDown) {
        return Err(String::from("P16: a confined labelled caller read a shared unlabelled volume"));
    }
    Ok(())
}
