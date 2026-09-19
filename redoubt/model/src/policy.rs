//! Property tests for the steward's policy (steward.rs): random policy operations, with the
//! policy's properties and the kernel's invariants checked after each one.
//!
//! | Property | Statement | Source |
//! | --- | --- | --- |
//! | P1 sessions | a session's budget is under its principal's (a sub-agent's under its agent's), with its account; labels are none or one the principal owns | CONTAINMENT.md, "Sessions and vaults" |
//! | P2 login | a login used one of the principal's login keys, never one `keyd` holds | CAPABILITIES.md, "The powerbox and approvals" |
//! | P3 approvals | an approval came through `approve@box` with the approver's approval key, named the frozen content's hash, and granted no label the approver lacks | CAPABILITIES.md, "Binding", "Limits and labels" |
//! | P4 screens | an approver sees only its own requests, labelled ones only if it owns every label; rendered text is printable ASCII with capped free text | CAPABILITIES.md, "Rendering"; QUESTIONS 34 |
//! | P5 cap | at most `PENDING_CAP` pending requests per (account, label set), all of live sessions | CONTAINMENT.md, "The shared server library"; QUESTIONS 17 |
//! | P6 declassification | what is copied out is exactly the snapshot taken at submission | CONTAINMENT.md, "Declassification" |
//! | P7 blame | an account is logged out exactly when three server crashes blamed on it (by the kernel's exit notices) fall within ten minutes; no other account's sessions are touched | CONTAINMENT.md, "Crash blame"; INIT.md |
//! | P8 labelled sessions | a labelled session starts nothing; it only submits requests | CONTAINMENT.md, "The shared server library" (steward) |
//! | P9 leases | an agent's budget has a deadline at most `MAX_LEASE` away; a sub-agent sits in its agent's budget and ends no later; an expired lease is gone | CAPABILITIES.md, "Agents"; QUESTIONS 33 |
//! | P10 non-interference | a vault session's work (item writes, requests, calls to a shared server) changes nothing an unlabelled session observes: its results, and the usage of every principal's and `users`' budget | PLAN.md, attack suite "no leaky state"; CONTAINMENT.md |

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
            PolicyOp::WriteItem { session: session(rng), item: rng.below(3), bytes }
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
        65..=79 => PolicyOp::Work { session: session(rng) },
        80..=83 => PolicyOp::Serve,
        84..=85 => PolicyOp::Hold,
        86..=87 => PolicyOp::Crash,
        88..=91 => PolicyOp::CrashServing { session: session(rng) },
        _ => PolicyOp::Tick {
            dt: match rng.below(3) {
                0 => rng.range(1, 50) * SLICE,
                1 => rng.range(1, 4) * 60_000_000,
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
    /// Blame times per account.
    pub ghost_blames: BTreeMap<u64, Vec<u64>>,
    /// The account of the message the server holds open (from what `hold` returned), if any.
    held: Option<u64>,
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
            PolicyOp::WriteItem { session, item, bytes } => {
                format!("{:?}", self.st.write_item(*session, *item, bytes.clone()))
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
            PolicyOp::Serve => {
                self.st.serve();
                self.held = None;
                String::from("ok")
            }
            PolicyOp::Hold => {
                if let Some(m) = self.st.hold() {
                    self.held = Some(m.account);
                }
                String::from("ok")
            }
            PolicyOp::Crash => {
                let account = self.held.take().unwrap_or(0);
                self.st.crash_server();
                self.check_blame(account, now, audit_from, &sessions_before)?;
                String::from("ok")
            }
            PolicyOp::CrashServing { session } => {
                let r = self.st.work(*session);
                self.st.poll();
                if let Some(m) = self.st.hold() {
                    self.held = Some(m.account);
                }
                let account = self.held.take().unwrap_or(0);
                self.st.crash_server();
                self.check_blame(account, now, audit_from, &sessions_before)?;
                format!("{r:?}")
            }
            PolicyOp::Tick { dt } => {
                self.st.tick(*dt);
                String::from("ok")
            }
        };
        self.st.poll();
        self.check(audit_from)?;
        Ok(obs)
    }

    /// P7: the crash was blamed on `account` (the account of the message the server held), and
    /// sessions were logged out exactly when three such blames fall within ten minutes.
    fn check_blame(
        &mut self,
        account: u64,
        now: u64,
        audit_from: usize,
        before: &BTreeMap<u64, Session>,
    ) -> Result<(), String> {
        let blamed: Vec<u64> = self.st.audit[audit_from..]
            .iter()
            .filter_map(|a| match a {
                Audit::Blamed { account, .. } => Some(*account),
                _ => None,
            })
            .collect();
        if account != 0 && !blamed.contains(&account) {
            return Err(format!("P7: the server crashed serving account {account}, which was not blamed"));
        }
        if account == 0 && !blamed.is_empty() {
            return Err(format!("P7: a crash serving nobody blamed {blamed:?}"));
        }
        if account == 0 {
            return Ok(());
        }
        let times = self.ghost_blames.entry(account).or_default();
        times.push(now);
        times.retain(|t| now - *t < BLAME_WINDOW);
        let logout = times.len() >= BLAME_COUNT;
        if logout {
            times.clear();
        }
        let principal = self.st.principals.iter().position(|p| p.spec.account == account);
        for (id, s) in before {
            let mine = Some(s.principal) == principal;
            let alive = self.st.sessions.contains_key(id) || !self.st.k.budgets.contains_key(&s.budget);
            let still = self.st.sessions.contains_key(id);
            if mine && logout && still {
                return Err(format!(
                    "P7: account {account} blamed 3 times in 10 minutes, session {id} survived"
                ));
            }
            if (!mine || !logout) && !alive {
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
        let st = &self.st;
        self.checker.check(&st.k)?;
        for s in st.sessions.values() {
            let p = &st.principals[s.principal];
            let b = st.k.budgets.get(&s.budget).ok_or(format!("P1: session {} has no budget", s.id))?;
            let under = st.k.is_descendant_or_self(s.budget, p.budget) && s.budget != p.budget;
            if !under || b.account != p.spec.account || b.labels != s.labels {
                return Err(format!("P1: session {}'s budget is not its principal's", s.id));
            }
            if s.kind == SessionKind::Login && b.parent != Some(p.budget) {
                return Err(format!("P1: login session {} is not directly under its principal", s.id));
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
                Audit::Approved { id, principal, key, hash } => {
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
                Audit::Declassified { id, bytes, .. } => {
                    let want = self.ghost_requests.get(id).and_then(|(_, b)| b.clone());
                    if want.as_ref() != Some(bytes) {
                        return Err(format!(
                            "P6: declassified {bytes:?}, the snapshot at submission was {want:?}"
                        ));
                    }
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
        // The steward's slot 1 is `users`.
        let shared: Vec<u64> = core::iter::once(1).chain(with.st.principals.iter().map(|p| p.h)).collect();
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
