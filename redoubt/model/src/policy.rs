//! Property tests for the steward's policy (steward.rs): random policy operations, with the
//! policy's properties and the kernel's invariants checked after each one.
//!
//! | Property | Statement (source) |
//! | --- | --- |
//! | P1 sessions | a session's budget is under its principal's, with its account; labels are none or one the principal owns (CONTAINMENT.md, vaults) |
//! | P2 login | a login used one of the principal's login keys, never one `keyd` holds (CAPABILITIES.md) |
//! | P3 approvals | an approval came through `approve@box` with the approver's approval key, named the frozen content's hash, and granted no label the approver lacks |
//! | P4 screens | an approver sees only its own requests, labelled ones only if it owns every label; rendered text has no control characters and capped free text |
//! | P5 cap | at most `PENDING_CAP` pending requests per account |
//! | P6 declassification | what is copied out is exactly the snapshot taken at submission |
//! | P7 blame | an account is logged out exactly when three crashes blamed on it fall within ten minutes; no other account's sessions are touched |
//! | P8 labelled sessions | a labelled session starts nothing; it only submits requests |
//! | P9 leases | an agent's budget has a deadline, and is gone once it passes |
//! | P10 non-interference | a vault session's work changes nothing another principal observes (request ids, results, usage) |

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::check::Failure;
use crate::gen::Rng;
use crate::invariants;
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
    Login { principal: usize, label: Option<u64>, key: u64 },
    EndSession { session: u64 },
    StartAgent { session: u64, lease: u64 },
    WriteItem { session: u64, item: u64, bytes: Vec<u8> },
    Submit { session: u64, content: Content, reason: String },
    Pending { principal: usize },
    Approve { principal: usize, key: u64, request: ReqRef, hash: HashRef },
    Deny { principal: usize, key: u64, request: ReqRef },
    Blame { account: u64 },
    KeydAdd { key: u64 },
    Usage { principal: usize },
    Tick { dt: u64 },
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
        pages: 120,
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
        .map(|_| match rng.below(20) {
            0 => '\u{1b}',
            1 => '\n',
            2 => '"',
            3 => '\\',
            4 => '\u{7}',
            _ => (b'a' + rng.below(26) as u8) as char,
        })
        .collect()
}

/// A random policy operation, biased toward what exists.
pub fn random_op(st: &Steward, subs: &[ReqRef], rng: &mut Rng) -> PolicyOp {
    let sessions: Vec<u64> = st.sessions.keys().copied().collect();
    let session = |rng: &mut Rng| rng.pick(&sessions).unwrap_or(rng.range(1, 20));
    let principal = rng.below(st.principals.len() as u64) as usize;
    let request = |rng: &mut Rng| rng.pick(subs).unwrap_or(ReqRef { session: 1, nth: 0 });
    match rng.below(100) {
        0..=14 => {
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
        15..=18 => PolicyOp::EndSession { session: session(rng) },
        19..=26 => PolicyOp::StartAgent { session: session(rng), lease: rng.range(1, 400) * SLICE },
        27..=36 => {
            let len = if rng.pct(85) { rng.below(40) } else { rng.range(200, 300) };
            let bytes = (0..len).map(|_| if rng.pct(97) { b'a' + rng.below(26) as u8 } else { 7 }).collect();
            PolicyOp::WriteItem { session: session(rng), item: rng.below(3), bytes }
        }
        37..=56 => {
            let content = match rng.below(3) {
                0 => Content::Note { what: text(rng, 90) },
                1 => Content::AgentWithLabel { label: rng.range(7, 9), lease: rng.range(1, 200) * SLICE },
                _ => Content::Declassify { label: rng.range(7, 9), item: rng.below(3) },
            };
            PolicyOp::Submit { session: session(rng), content, reason: text(rng, 100) }
        }
        57..=62 => PolicyOp::Pending { principal },
        63..=76 => {
            let good = st.principals[principal].spec.approval_keys[0];
            let key = if rng.pct(85) { good } else { rng.pick(&ALL_KEYS).unwrap() };
            let hash = match rng.below(10) {
                0 => HashRef::Of(request(rng)),
                1 => HashRef::Literal(rng.next_u64()),
                _ => HashRef::Own,
            };
            PolicyOp::Approve { principal, key, request: request(rng), hash }
        }
        77..=79 => {
            let key = st.principals[principal].spec.approval_keys[0];
            PolicyOp::Deny { principal, key, request: request(rng) }
        }
        80..=87 => {
            PolicyOp::Blame { account: if rng.pct(90) { st.principals[principal].spec.account } else { 0 } }
        }
        88..=89 => PolicyOp::KeydAdd { key: rng.pick(&ALL_KEYS).unwrap() },
        90..=91 => PolicyOp::Usage { principal },
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

/// A run of policy operations with its own record of what was submitted, independent of the
/// steward's.
pub struct Run {
    pub st: Steward,
    /// (session, nth) -> request id, as the steward returned it.
    pub submitted: BTreeMap<(u64, u64), u64>,
    pub subs: Vec<ReqRef>,
    /// Request id -> (hash from the steward's screen at submission, declassified bytes then).
    pub ghost_requests: BTreeMap<u64, (u64, Option<Vec<u8>>)>,
    /// Blame times per account, and the accounts that must be logged out.
    pub ghost_blames: BTreeMap<u64, Vec<u64>>,
    pub per_session: BTreeMap<u64, u64>,
}

impl Run {
    pub fn new(mutation: Option<Mutation>, secret: u64) -> Run {
        Run {
            st: Steward::new(&manifest(), secret, mutation).expect("the test manifest is valid"),
            submitted: BTreeMap::new(),
            subs: Vec::new(),
            ghost_requests: BTreeMap::new(),
            ghost_blames: BTreeMap::new(),
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
        let obs = match op {
            PolicyOp::Login { principal, label, key } => {
                let name = self.st.principals[*principal].spec.name.clone();
                format!("{:?}", self.st.login(&name, *label, *key))
            }
            PolicyOp::EndSession { session } => format!("{:?}", self.st.end_session(*session)),
            PolicyOp::StartAgent { session, lease } => {
                let labelled = self.st.sessions.get(session).is_some_and(|s| !s.labels.is_empty());
                let r = self.st.start_agent(*session, *lease);
                if labelled && r.is_ok() {
                    return Err(format!("P8: labelled session {session} started an agent"));
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
                    // An id is shown only to its submitter: record whether it is fresh, not its value.
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
                    if r.text.chars().any(|c| c.is_control()) {
                        return Err(format!("P4: rendered request {} contains control characters", r.id));
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
            PolicyOp::Blame { account } => {
                self.st.blame(*account);
                let now = self.st.k.now;
                if *account != 0 {
                    let times = self.ghost_blames.entry(*account).or_default();
                    times.push(now);
                    times.retain(|t| now - *t < BLAME_WINDOW);
                    let logout = times.len() >= BLAME_COUNT;
                    if logout {
                        times.clear();
                    }
                    let principal = self.st.principals.iter().position(|p| p.spec.account == *account);
                    for (id, s) in &sessions_before {
                        let mine = Some(s.principal) == principal;
                        let alive =
                            self.st.sessions.contains_key(id) || !self.st.k.budgets.contains_key(&s.budget);
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
                }
                String::from("ok")
            }
            PolicyOp::KeydAdd { key } => {
                self.st.keyd_add(*key);
                String::from("ok")
            }
            PolicyOp::Usage { principal } => {
                let h = self.st.principals[*principal].h;
                format!("{:?}", self.st.usage(h))
            }
            PolicyOp::Tick { dt } => {
                self.st.tick(*dt);
                String::from("ok")
            }
        };
        self.check(audit_from)?;
        Ok(obs)
    }

    /// P1-P6 and P9 on the state, and on the audit entries this step added; the kernel's
    /// invariants on the kernel underneath.
    fn check(&self, audit_from: usize) -> Result<(), String> {
        let st = &self.st;
        invariants::check(&st.k)?;
        for s in st.sessions.values() {
            let p = &st.principals[s.principal];
            let b = st.k.budgets.get(&s.budget).ok_or(format!("P1: session {} has no budget", s.id))?;
            if b.parent != Some(p.budget) || b.account != p.spec.account || b.labels != s.labels {
                return Err(format!("P1: session {}'s budget is not its principal's", s.id));
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
        let mut pending: BTreeMap<u64, usize> = BTreeMap::new();
        for r in st.requests.values() {
            *pending.entry(r.account).or_default() += 1;
        }
        if let Some((a, n)) = pending.iter().find(|(_, n)| **n > PENDING_CAP) {
            return Err(format!("P5: account {a} has {n} pending requests"));
        }
        for e in &st.audit[audit_from..] {
            match e {
                Audit::Login { principal, key, .. }
                    if (!st.principals[*principal].spec.login_keys.contains(key)
                        || st.keyd.contains(key)) =>
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
                Audit::AgentStarted { sponsor, labels, .. }
                    if !labels.iter().all(|l| st.principals[*sponsor].spec.owned_labels.contains(l)) =>
                {
                    return Err(format!(
                        "P3: an agent labelled {labels:?} started for a principal owning fewer"
                    ));
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
    for i in 0..rng.range(20, 120) {
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

/// P10 (CONTAINMENT.md, the no-leaky-state attack case): one sequence runs twice, the second time
/// without the work of the vault sessions (their item writes and submissions). Everything another
/// principal observes must be the same: every result it gets, and the usage of its own top
/// budget (the vault owner's own usage is left out: see the cap, below).
///
/// The observer is another principal: requests of the vault's own principal count against one
/// per-account cap whatever their labels, so an unlabelled session of the same principal can see
/// its vault's pending requests through that cap (README.md, spec problems). Approvals of vault
/// requests are left out of the sequence; approving is declassifying by design.
pub fn steward_noninterference(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    let mut rng = Rng::new(seed);
    let secret = rng.next_u64();
    let fail =
        |message: String| Failure { family: "steward_noninterference", seed, message, ops: Vec::new() };
    // Build the sequence on a first run, recording which ops are vault work.
    let mut first = Run::new(mutation, secret);
    let mut ops: Vec<(PolicyOp, bool)> = Vec::new();
    let mut vault_sessions: BTreeSet<u64> = BTreeSet::new();
    for _ in 0..rng.range(20, 100) {
        let op = random_op(&first.st, &first.subs, &mut rng);
        let vault = match &op {
            PolicyOp::WriteItem { session, .. } | PolicyOp::Submit { session, .. } => {
                vault_sessions.contains(session)
            }
            // Approving or denying a vault request, blaming, and ending sessions are left out:
            // they are the owner's decisions, not the vault's work.
            PolicyOp::Approve { request, .. } | PolicyOp::Deny { request, .. } => {
                if vault_sessions.contains(&request.session) {
                    continue;
                }
                false
            }
            PolicyOp::EndSession { session } if vault_sessions.contains(session) => continue,
            PolicyOp::Blame { .. } => continue,
            _ => false,
        };
        let before = first.st.sessions.keys().copied().collect::<BTreeSet<u64>>();
        first.apply(&op).map_err(fail)?;
        if let PolicyOp::Login { label: Some(_), .. } = op {
            for s in first.st.sessions.keys() {
                if !before.contains(s) {
                    vault_sessions.insert(*s);
                }
            }
        }
        ops.push((op, vault));
    }
    // The observer: someone other than the vault owners.
    let owners: BTreeSet<usize> =
        vault_sessions.iter().filter_map(|s| first.st.sessions.get(s)).map(|s| s.principal).collect();
    let observers: Vec<usize> = (0..first.st.principals.len()).filter(|p| !owners.contains(p)).collect();
    let actor = |r: &Run, op: &PolicyOp| -> Option<usize> {
        match op {
            PolicyOp::Login { principal, .. }
            | PolicyOp::Pending { principal }
            | PolicyOp::Usage { principal } => Some(*principal),
            PolicyOp::Approve { principal, .. } | PolicyOp::Deny { principal, .. } => Some(*principal),
            PolicyOp::EndSession { session }
            | PolicyOp::StartAgent { session, .. }
            | PolicyOp::WriteItem { session, .. }
            | PolicyOp::Submit { session, .. } => r.st.sessions.get(session).map(|s| s.principal),
            _ => None,
        }
    };
    let mut with = Run::new(mutation, secret);
    let mut without = Run::new(mutation, secret);
    for (i, (op, vault)) in ops.iter().enumerate() {
        let who = actor(&with, op);
        let a = with.apply(op).map_err(fail)?;
        if *vault {
            continue;
        }
        let b = without.apply(op).map_err(fail)?;
        let observed = who.is_some_and(|p| observers.contains(&p));
        if observed && a != b {
            return Err(fail(format!(
                "P10: op {i} ({op:?}) showed {a:?} with the vault's work and {b:?} without"
            )));
        }
        // Ids of the observers' own requests.
        for (key, id) in &with.submitted {
            let mine = with.st.sessions.get(&key.0).is_some_and(|s| observers.contains(&s.principal));
            if mine && without.submitted.get(key) != Some(id) {
                return Err(fail(format!(
                    "P10: session {}'s request {} got a different id without the vault's work",
                    key.0, key.1
                )));
            }
        }
        for &p in &observers {
            let h = with.st.principals[p].h;
            if with.st.usage(h) != without.st.usage(h) {
                return Err(fail(format!(
                    "P10: principal {p}'s budget usage depends on the vault's work (op {i})"
                )));
            }
        }
    }
    Ok(())
}
