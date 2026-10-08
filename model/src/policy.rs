//! Property tests for the steward's policy core, as the model embeds it (steward.rs): random
//! policy operations, turned into the core's events, with the policy's properties and the
//! kernel's invariants checked after each. The checks read the core's state through its read-only
//! `inspect` API, the kernel model, and what the family itself submitted and was shown.
//!
//! | Property | Statement | Source |
//! | --- | --- | --- |
//! | P1 sessions | a session's or a lease's budget is carved from its domain's fixed sub-budget (a sub-agent's from its agent's), with its principal's account and its domain's labels, which the principal owns | servers/steward.md, "Fixed sub-budgets per label set" |
//! | P2 login | a login used one of the principal's login keys, never one `keyd` holds | servers/steward.md, "Authentication and sessions" |
//! | P3 approvals | an approval channel opened with the principal's approval key; an approval was answered on the channel that rendered the request last, named the hash it showed, which never changed, and granted no label the approver lacks | servers/steward.md R38 |
//! | P4 screens | a channel sees only its own principal's requests, and labelled ones only if it owns every label; rendered text is printable ASCII with capped free text; a labelled request shows none of its free text; an approval-waiting notice reaches only sessions whose labels include the request's | servers/steward.md, "The powerbox and approvals"; R38 |
//! | P5 cap | at most `PENDING_CAP` pending requests per domain, all of live sessions and agents; each holds at most its fair share | servers/steward.md, "The powerbox and approvals"; servers/serving.md R26 |
//! | P6 crossings | a declassification is submitted from a session with exactly the item's labels and a push from an unlabelled one; what is copied out or pushed is exactly the snapshot taken at submission, a declassified item at most `DECLASSIFY_MAX` bytes of printable text, read or written through a budget with exactly the labelled side's labels | servers/steward.md R42 |
//! | P7 blame | a domain's sessions and leases end exactly when three server crashes blamed on it (by the kernel's exit notices) fall within ten minutes; no others are touched; none of it starts for the next ten minutes | servers/steward.md R40; servers/init.md, "Restarts and reboots" |
//! | P8 labelled sessions | a labelled session or agent starts nothing; it only submits requests | servers/steward.md, "Authentication and sessions" |
//! | P9 leases | an agent's budget has a deadline at most `MAX_LEASE` away; a sub-agent sits in its agent's budget and ends no later; an expired lease is gone | servers/steward.md R39 |
//! | P10 non-interference | a vault session's work (item writes, requests, calls to a shared server) changes nothing an unlabelled session observes: its results, the usage of `users`, of every principal's budget and unlabelled sub-budget, and the audit records an unlabelled reader may read | servers/steward.md R37 |
//! | P11 writes | every write to an item is through a budget with exactly the item's labels; the steward's own write goes only to the unlabelled volume | servers/steward.md R42 |
//! | P12 system budgets | only `init` and the steward hold a handle to a system-class budget; a session's connections are narrowed to a revocation scope inside its budget | servers/steward.md R41; servers/init.md R33 |
//! | P13 leases end | a lease's sponsor can always end it from an unlabelled session, and nothing else can | servers/steward.md R39 |
//! | P14 audit | every audit record is signed through `keyd`'s audit purpose | servers/steward.md, "The audit log" |
//! | P15 shares | a chain of self-mints spends one admission share (`connection_lineage`) | servers/serving.md R26 |
//! | P16 confined reads | a confined labelled caller reads no shared unlabelled volume (`confined_read_observation`) | servers/init.md, "The confinement check" |
//! | P17 contexts | a domain holds at most one live session of a context's name; a login with a name that is not one starts nothing; a wrong key, a label set not the principal's and a context that is not a name are refused alike | servers/steward.md R79 |

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use redoubt_steward::Policy;
use redoubt_steward::audit::Record;
use redoubt_steward::consts::{BLAME_COUNT, BLAME_WINDOW, DECLASSIFY_MAX, FIELD_CAP, MAX_LEASE, PENDING_CAP};
use redoubt_steward::domain::Labels;
use redoubt_steward::effect::{Answer, Kind, Notice, Notified, Output, Refusal};
use redoubt_steward::event::{Content, Event};
use redoubt_steward::inspect;
use redoubt_steward::manifest::{Limits, Manifest, PrincipalSpec, Sizes};
use redoubt_steward::render::{printable, sanitize};

use crate::check::Failure;
use crate::gen::Rng;
use crate::invariants::Checker;
use crate::mutation::Mutation;
use crate::serving::ConnectionShares;
use crate::spec::SLICE;
use crate::steward::{Caller, Denied, Steward};

/// A request, named by who submitted it and when, so a sequence means the same thing when
/// replayed with some operations removed (P10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReqRef {
    pub session: u64,
    pub nth: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashRef {
    /// The hash the request's screen showed.
    Own,
    /// Another request's hash (a swapped approval).
    Of(ReqRef),
    Literal(u64),
}

/// The operations. A session is named by its id, an agent by its lease's; an approval channel by
/// the order it was opened in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyOp {
    Login {
        principal: usize,
        labels: Vec<u64>,
        /// The context's name, empty for the default one.
        context: String,
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
        labels: Vec<u64>,
        item: u64,
        bytes: Vec<u8>,
    },
    Submit {
        session: u64,
        content: Content,
        reason: String,
    },
    /// `ssh approve@box` with `key`.
    Open {
        principal: usize,
        key: u64,
    },
    Close {
        channel: usize,
    },
    Pending {
        channel: usize,
    },
    Approve {
        channel: usize,
        request: ReqRef,
        hash: HashRef,
    },
    Deny {
        channel: usize,
        request: ReqRef,
    },
    Usage {
        principal: usize,
    },
    /// The session's process calls the server.
    Work {
        session: u64,
    },
    /// Session or agent `by` ends lease `lease`.
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

/// The test manifest: alice owns labels 7 and 8 and works under {}, {7} and {8}; bob owns 9 and
/// works under {}, {9} and {7}, which he does not own; carol owns none. Keys: login 11/21/31,
/// approval 12/22/32; keyd holds 100 and 101.
pub fn manifest() -> Manifest {
    let p = |name: &str, account, login, approval, owned: &[u64], sets: &[&[u64]]| PrincipalSpec {
        name: String::from(name),
        account,
        login_keys: vec![login],
        approval_keys: vec![approval],
        owned: owned.to_vec(),
        label_sets: sets.iter().map(|s| s.to_vec()).collect(),
        top: Limits { pages: 500, processes: 12, weight: 100 },
    };
    let l = |pages, processes, weight| Limits { pages, processes, weight };
    let page = crate::kernel::Costs::default().budget;
    Manifest {
        principals: vec![
            p("alice", 1001, 11, 12, &[7, 8], &[&[], &[7], &[8]]),
            p("bob", 1002, 21, 22, &[9], &[&[], &[9], &[7]]),
            p("carol", 1003, 31, 32, &[], &[&[]]),
        ],
        keyd_keys: vec![100, 101],
        servers: 1,
        sizes: Sizes {
            session: l(40, 1, 5),
            agent: l(40, 2, 4),
            sub_agent: l(16, 1, 1),
            crossing: l(page, 0, 0),
            budget_cost: page,
        },
    }
}

const ALL_KEYS: [u64; 8] = [11, 12, 21, 22, 31, 32, 100, 101];
/// The contexts a login names: mostly a few good names, so a domain holds several sessions, now
/// and then one that is not a name.
const CONTEXTS: [&str; 8] = ["", "", "a", "b", "c", "work", "Work", "a.b"];
const LABEL_SETS: [&[u64]; 4] = [&[], &[7], &[8], &[9]];

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

/// A label set: usually `mine`, now and then any.
fn labels(rng: &mut Rng, mine: &[u64]) -> Vec<u64> {
    if rng.pct(70) { mine.to_vec() } else { LABEL_SETS[rng.below(4) as usize].to_vec() }
}

/// An approval channel as the family opened it.
#[derive(Clone, Copy, Debug)]
pub struct Chan {
    pub id: u64,
    pub principal: usize,
    pub key: u64,
}

/// What the family submitted: the submitter, the content and reason, and what its snapshot must
/// be (the item at submission, for a declassification or a push).
#[derive(Clone, Debug)]
pub struct Submitted {
    pub by: Caller,
    pub content: Content,
    pub reason: String,
    pub snapshot: Option<Vec<u8>>,
}

/// A random policy operation, biased toward what exists.
pub fn random_op(run: &Run, rng: &mut Rng) -> PolicyOp {
    let st = &run.st;
    let callers = st.callers();
    let ids: Vec<u64> = callers.iter().map(|c| c.id).collect();
    let session = |rng: &mut Rng| rng.pick(&ids).unwrap_or(rng.range(1, 20));
    let mine = |id: u64| {
        callers.iter().find(|c| c.id == id).map_or(Vec::new(), |c| c.domain.labels().as_slice().to_vec())
    };
    let fixed = inspect::fixed(&st.store);
    let principal = rng.below(fixed.principals.len() as u64) as usize;
    let channel = |rng: &mut Rng| rng.below(run.channels.len().max(1) as u64) as usize;
    let request = |rng: &mut Rng| rng.pick(&run.subs).unwrap_or(ReqRef { session: 1, nth: 0 });
    // A channel of the request's principal, usually the one that rendered it last.
    let answering = |rng: &mut Rng, r: ReqRef| {
        let id = run.resolve(r);
        let last = run.shown.get(&id).map(|s| s.1);
        let of = run.ghost.get(&id).map(|g| g.by.principal);
        let pick = |want: &dyn Fn(&Chan) -> bool, rng: &mut Rng| {
            let found: Vec<usize> = (0..run.channels.len()).filter(|i| want(&run.channels[*i])).collect();
            rng.pick(&found)
        };
        match rng.below(10) {
            0..=5 => pick(&|c: &Chan| Some(c.id) == last, rng),
            6..=8 => pick(&|c: &Chan| Some(c.principal) == of, rng),
            _ => None,
        }
        .unwrap_or_else(|| channel(rng))
    };
    match rng.below(100) {
        0..=11 => {
            let sets = &fixed.principals[principal].domains;
            let labels = match rng.below(10) {
                0..=4 => Vec::new(),
                5..=8 => sets[rng.below(sets.len() as u64) as usize].labels().as_slice().to_vec(),
                _ => LABEL_SETS[rng.below(4) as usize].to_vec(),
            };
            let good = fixed.principals[principal].login_keys[0];
            let key = if rng.pct(80) { good } else { rng.pick(&ALL_KEYS).unwrap() };
            let context = String::from(rng.pick(&CONTEXTS).unwrap());
            PolicyOp::Login { principal, labels, context, key }
        }
        12..=14 => PolicyOp::EndSession { session: session(rng) },
        15..=21 => PolicyOp::StartAgent { session: session(rng), lease: lease(rng) },
        22..=28 => {
            let s = session(rng);
            let len = if rng.pct(85) { rng.below(40) } else { rng.range(200, 300) };
            let bytes = (0..len).map(|_| if rng.pct(97) { b'a' + rng.below(26) as u8 } else { 7 }).collect();
            PolicyOp::WriteItem { session: s, labels: labels(rng, &mine(s)), item: rng.below(3), bytes }
        }
        29..=43 => {
            let s = session(rng);
            let content = match rng.below(4) {
                0 => Content::Note { what: text(rng, 90) },
                1 => {
                    // A labelled caller mostly asks in its own set, sometimes unlabelled or
                    // another (R37); an unlabelled one mostly for a vault agent.
                    let own = mine(s);
                    let labels = match rng.below(10) {
                        0..=6 if !own.is_empty() => own,
                        0..=7 => LABEL_SETS[rng.range(1, 3) as usize].to_vec(),
                        _ => LABEL_SETS[rng.below(4) as usize].to_vec(),
                    };
                    Content::Agent { labels, lease: lease(rng) }
                }
                2 => Content::Declassify { labels: labels(rng, &mine(s)), item: rng.below(3) },
                _ => Content::Push {
                    source: rng.below(3),
                    target: LABEL_SETS[rng.range(1, 3) as usize].to_vec(),
                    item: rng.below(3),
                },
            };
            PolicyOp::Submit { session: s, content, reason: text(rng, 100) }
        }
        44..=46 => {
            let good = fixed.principals[principal].approval_keys[0];
            let key = if rng.pct(85) { good } else { rng.pick(&ALL_KEYS).unwrap() };
            PolicyOp::Open { principal, key }
        }
        47 => PolicyOp::Close { channel: channel(rng) },
        48..=53 => PolicyOp::Pending { channel: channel(rng) },
        54..=60 => {
            let r = request(rng);
            let hash = match rng.below(10) {
                0 => HashRef::Of(request(rng)),
                1 => HashRef::Literal(rng.next_u64()),
                _ => HashRef::Own,
            };
            PolicyOp::Approve { channel: answering(rng, r), request: r, hash }
        }
        61..=62 => {
            let r = request(rng);
            PolicyOp::Deny { channel: answering(rng, r), request: r }
        }
        63..=64 => PolicyOp::Usage { principal },
        65..=76 => PolicyOp::Work { session: session(rng) },
        77..=79 => PolicyOp::Serve,
        80..=81 => PolicyOp::Hold,
        82..=83 => PolicyOp::Crash,
        84..=88 => PolicyOp::CrashServing { session: session(rng) },
        89..=91 => {
            // Usually a lease and an unlabelled session of its account.
            let leases: Vec<u64> = callers.iter().filter(|c| c.kind == Kind::Lease).map(|c| c.id).collect();
            let lease = rng.pick(&leases).unwrap_or_else(|| session(rng));
            let account = callers.iter().find(|c| c.id == lease).map(|c| c.domain.account());
            let sponsors: Vec<u64> = callers
                .iter()
                .filter(|c| c.kind == Kind::Session && Some(c.domain.account()) == account && !c.labelled())
                .map(|c| c.id)
                .collect();
            let by =
                if rng.pct(80) { rng.pick(&sponsors).unwrap_or_else(|| session(rng)) } else { session(rng) };
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

/// A run of policy operations with its own record of what was submitted, shown and blamed,
/// independent of the core's.
pub struct Run {
    pub st: Steward,
    checker: Checker,
    /// (session, nth) -> request id, as the steward returned it.
    pub submitted: BTreeMap<(u64, u64), u64>,
    pub subs: Vec<ReqRef>,
    /// Request id -> what was submitted.
    pub ghost: BTreeMap<u64, Submitted>,
    /// Request id -> the hash its first screen showed, and the channel that rendered it last.
    pub shown: BTreeMap<u64, ([u8; 32], u64)>,
    /// The approval channels, in the order opened.
    pub channels: Vec<Chan>,
    /// Blame times per (account, label set).
    pub ghost_blames: BTreeMap<(u64, Vec<u64>), Vec<u64>>,
    /// The account and labels of the call the server works on (from what `hold` returned), if any.
    held: Option<(u64, Vec<u64>)>,
    /// (account, label set)s locked out, and when their lockout ends (P7).
    pub ghost_locked: BTreeMap<(u64, Vec<u64>), u64>,
    pub per_session: BTreeMap<u64, u64>,
    /// The properties' instances the checks met: a check holds non-vacuously only from the first
    /// sequence that gives it one (the coverage instrument, model/tests/steward_reach.rs).
    pub reached: BTreeSet<&'static str>,
}

fn hash_of(x: u64) -> [u8; 32] {
    let mut h = [0; 32];
    h[..8].copy_from_slice(&x.to_le_bytes());
    h
}

impl Run {
    pub fn new(mutation: Option<Mutation>, secret: u64) -> Run {
        Run::with_policy(mutation, secret, crate::mutation::policy(mutation))
    }

    /// As `new`, with the core deciding by `policy` (`Steward::with_policy`).
    pub fn with_policy(mutation: Option<Mutation>, secret: u64, policy: Policy) -> Run {
        let st =
            Steward::with_policy(&manifest(), secret, mutation, policy).expect("the test manifest is valid");
        let checker = Checker::new(&st.k);
        Run {
            st,
            checker,
            submitted: BTreeMap::new(),
            subs: Vec::new(),
            ghost: BTreeMap::new(),
            shown: BTreeMap::new(),
            channels: Vec::new(),
            ghost_blames: BTreeMap::new(),
            held: None,
            ghost_locked: BTreeMap::new(),
            per_session: BTreeMap::new(),
            reached: BTreeSet::new(),
        }
    }

    fn resolve(&self, r: ReqRef) -> u64 {
        self.submitted
            .get(&(r.session, r.nth))
            .copied()
            .unwrap_or(r.session.wrapping_mul(7919).wrapping_add(r.nth))
    }

    /// The channel the family opened `n`th; its id is 0, which names none, if none was.
    fn channel(&self, n: usize) -> Chan {
        self.channels.get(n).copied().unwrap_or(Chan { id: 0, principal: 0, key: 0 })
    }

    /// Apply one op, check the properties and the kernel invariants, and return what the caller
    /// saw.
    pub fn apply(&mut self, op: &PolicyOp) -> Result<Obs, String> {
        let from = (self.st.audit.len(), self.st.outputs.len(), self.st.writes.len());
        let before: Vec<Caller> = self.st.callers();
        let now = self.st.k.now;
        let fixed = inspect::fixed(&self.st.store).clone();
        let obs = match op {
            PolicyOp::Login { principal, labels, context, key } => {
                let name = fixed.principals[*principal].name.clone();
                let r = self.st.login(&name, labels, context, *key);
                let named = context.is_empty() || redoubt_steward::manifest::name(context);
                if matches!(r, Some(Answer::Session { .. })) && !named {
                    return Err(format!("P17: a login named the context {context:?}, which is not a name"));
                }
                if r == Some(Answer::Refused(Refusal::InUse)) {
                    self.reached.insert("P17 a login to a live context");
                }
                // No enumeration: a wrong key, a label set not the principal's to log in under and
                // a context that is not a name are all refused alike.
                let p = &fixed.principals[*principal];
                let owned = Labels::new(labels)
                    .is_some_and(|l| p.owned.includes(&l) && p.domains.iter().any(|d| *d.labels() == l));
                let key_right = p.login_keys.contains(key) && !fixed.keyd.contains(key);
                if !(key_right && owned && named) {
                    if r != Some(Answer::Refused(Refusal::BadKey)) {
                        return Err(format!(
                            "P17: a wrong login {op:?} was answered {r:?}, not as a bad key"
                        ));
                    }
                    if key_right {
                        self.reached.insert("P17 a wrong label set or context refused as a bad key");
                    }
                }
                format!("{r:?}")
            }
            PolicyOp::EndSession { session } => format!("{:?}", self.st.end_session(*session)),
            PolicyOp::StartAgent { session, lease } => {
                let requester = self.st.caller(*session);
                let r = self.st.start_agent(*session, *lease);
                if requester.as_ref().is_some_and(|c| c.labelled()) {
                    self.reached.insert("P8 a labelled caller asks to start an agent");
                }
                if let (Some(req), Some(Answer::Lease { id, .. })) = (&requester, &r) {
                    if req.labelled() {
                        return Err(format!("P8: labelled {:?} {session} started an agent", req.kind));
                    }
                    let new = self.st.caller(*id).and_then(|c| self.st.budget_of(&c.object()));
                    let mine = self.st.budget_of(&req.object());
                    if req.kind == Kind::Lease {
                        let k = &self.st.k;
                        let (Some(new), Some(mine)) = (new, mine) else {
                            return Err(format!("P9: agent {session}'s sub-agent {id} has no budget"));
                        };
                        let deadline = |b: u64| k.budgets.get(&b).and_then(|x| x.deadline);
                        let under = k.is_descendant_or_self(new, mine);
                        let ends = deadline(new).is_some_and(|d| deadline(mine).is_some_and(|m| d <= m));
                        if !under || !ends {
                            return Err(format!(
                                "P9: agent {session}'s sub-agent is outside it or outlives it"
                            ));
                        }
                    }
                }
                format!("{r:?}")
            }
            PolicyOp::WriteItem { session, labels, item, bytes } => {
                format!("{:?}", self.st.write_item(*session, labels, *item, bytes.clone()))
            }
            PolicyOp::Submit { session, content, reason } => {
                // What a snapshot must contain, read from the volume before the steward acts.
                let volume = |labels: &[u64], item: u64| {
                    let labels = Labels::new(labels).unwrap_or_default().as_slice().to_vec();
                    self.st.volumes.get(&(labels, item)).cloned().unwrap_or_default()
                };
                let snapshot = match content {
                    Content::Declassify { labels, item } => Some(volume(labels, *item)),
                    Content::Push { source, .. } => Some(volume(&[], *source)),
                    _ => None,
                };
                let by = self.st.caller(*session);
                let r = self.st.submit(*session, content.clone(), reason);
                match (r, by) {
                    (Some(Answer::Request { id }), Some(by)) => {
                        self.submitted_checks(&by, from.1)?;
                        let nth = *self.per_session.entry(*session).or_default();
                        self.per_session.insert(*session, nth + 1);
                        self.submitted.insert((*session, nth), id);
                        self.subs.push(ReqRef { session: *session, nth });
                        let g = Submitted { by, content: content.clone(), reason: reason.clone(), snapshot };
                        self.ghost.insert(id, g);
                        // An id is shown only to its submitter: record that it is fresh, not its value.
                        format!("Ok(request {nth})")
                    }
                    (r, _) => format!("{r:?}"),
                }
            }
            PolicyOp::Open { principal, key } => {
                let p = &fixed.principals[*principal];
                let (id, r) = self.st.open_channel(&p.name, *key);
                self.channels.push(Chan { id, principal: *principal, key: *key });
                let approval = p.approval_keys.contains(key)
                    && !fixed.principals.iter().any(|q| q.login_keys.contains(key))
                    && !fixed.keyd.contains(key);
                if r == Some(Answer::Ok) && !approval {
                    return Err(format!("P3: {}'s approval channel opened with key {key}", p.name));
                }
                if r == Some(Answer::Ok) {
                    self.reached.insert("P3 a channel opens");
                }
                format!("{r:?}")
            }
            PolicyOp::Close { channel } => {
                self.st.close_channel(self.channel(*channel).id);
                String::from("ok")
            }
            PolicyOp::Pending { channel } => {
                let screens = self.st.pending(self.channel(*channel).id);
                format!("{} requests", screens.len())
            }
            PolicyOp::Approve { channel, request, hash } => {
                let ch = self.channel(*channel);
                let id = self.resolve(*request);
                let h = match hash {
                    HashRef::Own => self.shown.get(&id).map_or([0; 32], |s| s.0),
                    HashRef::Of(other) => self.shown.get(&self.resolve(*other)).map_or([1; 32], |s| s.0),
                    HashRef::Literal(x) => hash_of(*x),
                };
                let r = self.st.approve(ch.id, id, h);
                if r == Some(Answer::Ok) {
                    self.answered_checks(ch, id)?;
                    if self.shown.get(&id).map(|s| s.0) != Some(h) {
                        return Err(format!(
                            "P3: request {id} approved naming a hash its screen did not show"
                        ));
                    }
                }
                format!("{r:?}")
            }
            PolicyOp::Deny { channel, request } => {
                let ch = self.channel(*channel);
                let id = self.resolve(*request);
                let r = self.st.deny(ch.id, id);
                if r == Some(Answer::Ok) {
                    self.answered_checks(ch, id)?;
                }
                format!("{r:?}")
            }
            PolicyOp::Usage { principal } => {
                let account = fixed.principals[*principal].account.get();
                let h = self.st.tops[&account].1;
                format!("{:?}", self.st.usage(h))
            }
            PolicyOp::Work { session } => format!("{:?}", self.st.work(*session)),
            PolicyOp::EndLease { by, lease } => {
                let (b, l) = (self.st.caller(*by), self.st.caller(*lease));
                let leased = l.as_ref().is_some_and(|l| l.kind == Kind::Lease);
                let sponsor = b.as_ref().is_some_and(|b| {
                    b.kind == Kind::Session
                        && !b.labelled()
                        && l.as_ref().is_some_and(|l| l.domain.account() == b.domain.account())
                });
                let r = self.st.end_lease(*by, *lease);
                if leased && sponsor && (r != Some(Answer::Ok) || self.st.caller(*lease).is_some()) {
                    return Err(format!("P13: session {by} could not end its agent {lease}: {r:?}"));
                }
                if r == Some(Answer::Ok) && !sponsor {
                    return Err(format!(
                        "P13: {:?} {by}, not an unlabelled session of its sponsor, ended lease {lease}",
                        b.map(|b| b.domain)
                    ));
                }
                if leased {
                    self.reached.insert(if sponsor {
                        "P13 a sponsor ends its agent"
                    } else {
                        "P13 another caller tries to end an agent"
                    });
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
                self.check_blame(held, now, from.0, &before)?;
                String::from("ok")
            }
            PolicyOp::CrashServing { session } => {
                // The crash is the call's: the server answers what it takes until it takes this
                // session's call, and crashes serving it.
                let r = self.st.work(*session);
                self.st.poll();
                let mut held = (0, Vec::new());
                while let Some(m) = self.st.hold() {
                    if m.words[0] == *session {
                        held = (m.account, m.labels);
                        break;
                    }
                    self.st.reply(m.msg_id);
                }
                self.held = None;
                self.st.crash_server();
                self.check_blame(held, now, from.0, &before)?;
                format!("{r:?}")
            }
            PolicyOp::Tick { dt } => {
                self.st.tick(*dt);
                String::from("ok")
            }
        };
        self.st.poll();
        // P7: nothing of a locked-out domain starts within its window.
        let existed: BTreeSet<u64> = before.iter().map(|c| c.id).collect();
        for c in self.st.callers() {
            let key = (c.domain.account().get(), c.domain.labels().as_slice().to_vec());
            let locked = self.ghost_locked.get(&key).is_some_and(|until| now < *until);
            if locked && !existed.contains(&c.id) {
                return Err(format!("P7: {:?} {} of {key:?} started while it was locked out", c.kind, c.id));
            }
        }
        self.check(from)?;
        Ok(obs)
    }

    /// P5's fair share and P4's notices, after a submission by `by`.
    fn submitted_checks(&mut self, by: &Caller, outputs_from: usize) -> Result<(), String> {
        self.reached.insert("P5 a request is pending");
        let state = inspect::domain(&self.st.store, &by.domain).ok_or("P5: a request in no domain")?;
        let mine = state.requests.values().filter(|r| r.by.kind == by.kind && r.by.id == by.id).count();
        let share = (PENDING_CAP / (state.sessions.len() + state.leases.len()).max(1)).max(1);
        if mine > share {
            return Err(format!(
                "P5: {:?} {} holds {mine} pending requests, over its fair share {share}",
                by.kind, by.id
            ));
        }
        let index = inspect::index(&self.st.store);
        for o in &self.st.outputs[outputs_from..] {
            let Output::Notice { to, notice: Notice::ApprovalWaiting } = o else { continue };
            self.reached.insert("P4 an approval-waiting notice");
            match to {
                Notified::Session(b) => {
                    let labels = index.routes.get(b).map(|r| r.domain.labels().clone());
                    if !labels.as_ref().is_some_and(|l| l.includes(by.domain.labels())) {
                        return Err(format!(
                            "P4: an approval-waiting notice for labels {:?} reached badge {b} with labels {labels:?}",
                            by.domain.labels()
                        ));
                    }
                }
                Notified::Channel(c) => {
                    if !self.channels.iter().any(|x| x.id == *c && x.principal == by.principal) {
                        return Err(format!(
                            "P4: an approval-waiting notice reached another principal's channel {c}"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// P3: a request is answered only on the channel that rendered it last.
    fn answered_checks(&self, ch: Chan, id: u64) -> Result<(), String> {
        match self.shown.get(&id) {
            Some((_, last)) if *last == ch.id => Ok(()),
            _ => Err(format!("P3: request {id} answered on channel {}, which did not render it last", ch.id)),
        }
    }

    /// P7: the crash was blamed on `held` (the account and labels of the newest message the server
    /// held), and the domain's sessions and leases ended exactly when three such blames fall within
    /// ten minutes.
    fn check_blame(
        &mut self,
        held: (u64, Vec<u64>),
        now: u64,
        audit_from: usize,
        before: &[Caller],
    ) -> Result<(), String> {
        let (account, labels) = held;
        let blamed: Vec<(u64, Vec<u64>)> = self.st.audit[audit_from..]
            .iter()
            .filter(|a| *a.record() == Record::Blamed)
            .map(|a| (a.domain().account().get(), a.domain().labels().as_slice().to_vec()))
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
        self.reached.insert("P7 a crash is blamed");
        let times = self.ghost_blames.entry(key).or_default();
        times.push(now);
        times.retain(|t| now - *t < BLAME_WINDOW);
        let lockout = times.len() >= BLAME_COUNT;
        if lockout {
            self.reached.insert("P7 a domain is locked out");
            times.clear();
            self.ghost_locked.insert((account, labels.clone()), now + BLAME_WINDOW);
        }
        let after: BTreeSet<u64> = self.st.callers().iter().map(|c| c.id).collect();
        for c in before {
            let mine = c.domain.account().get() == account && c.domain.labels().as_slice() == labels;
            let still = after.contains(&c.id);
            if mine && lockout && still {
                return Err(format!(
                    "P7: account {account} with labels {labels:?} blamed 3 times in 10 minutes, {:?} {} survived",
                    c.kind, c.id
                ));
            }
            if (!mine || !lockout) && !still {
                return Err(format!(
                    "P7: blaming account {account} ended {:?} {} of {:?}",
                    c.kind, c.id, c.domain
                ));
            }
        }
        Ok(())
    }

    /// The properties on the state, and on the audit records, outputs and writes this step added;
    /// the kernel's invariants on the kernel underneath.
    fn check(
        &mut self,
        (audit_from, outputs_from, writes_from): (usize, usize, usize),
    ) -> Result<(), String> {
        if !self.st.audit_authentic() {
            return Err(String::from("P14: altered or unsigned audit record"));
        }
        if !self.st.audit.is_empty() {
            self.reached.insert("P14 an audit record");
        }
        if inspect::exited(&self.st.store) {
            return Err(String::from("the steward exited: an event its guarantee excludes"));
        }
        self.checker.check(&self.st.k)?;
        self.screens(outputs_from)?;
        let st = &self.st;
        let k = &st.k;
        let fixed = inspect::fixed(&st.store);
        let callers = st.callers();
        let budget = |c: &Caller| st.budget_of(&c.object());
        let lease_budgets: BTreeSet<u64> =
            callers.iter().filter(|c| c.kind == Kind::Lease).filter_map(budget).collect();
        for c in &callers {
            let p = &fixed.principals[c.principal];
            let what = format!("{:?} {} of {:?}", c.kind, c.id, c.domain);
            let id = budget(c).ok_or(format!("P1: {what} has no budget"))?;
            let b = k.budgets.get(&id).ok_or(format!("P9: {what}'s budget is gone, and it is not"))?;
            let top = st.tops[&p.account.get()].0;
            let under = k.is_descendant_or_self(id, top) && id != top;
            if !under || b.account != p.account.get() || b.labels != c.domain.labels().as_slice() {
                return Err(format!("P1: {what}'s budget is not its principal's, with its labels"));
            }
            // Carved from its domain's sub-budget, or (a sub-agent) from an agent's budget there.
            let sub = st.subs.get(&c.domain).map(|x| x.0);
            let in_agent = c.kind == Kind::Lease && b.parent.is_some_and(|x| lease_budgets.contains(&x));
            if b.parent != sub && !in_agent {
                return Err(format!("P1: {what} is not carved from its domain's sub-budget"));
            }
            self.reached.insert(match (c.kind, in_agent, c.labelled()) {
                (Kind::Lease, true, _) => "P1 a sub-agent",
                (Kind::Lease, false, _) => "P1 an agent",
                (_, _, true) => "P1 a labelled session",
                _ => "P1 an unlabelled session",
            });
            if !p.owned.includes(c.domain.labels()) {
                return Err(format!("P1: {what} carries labels its principal does not own"));
            }
            if c.kind == Kind::Lease {
                if b.deadline.is_none_or(|d| d <= k.now) {
                    return Err(format!("P9: {what} has no future deadline"));
                }
                let parent = b.parent.and_then(|x| k.budgets.get(&x)).and_then(|x| x.deadline);
                if in_agent && parent.is_none_or(|pd| b.deadline.is_some_and(|d| d > pd)) {
                    return Err(format!("P9: {what} outlives its agent"));
                }
            }
        }
        // P12: only init and the steward hold system budgets; connections are narrowed to scopes.
        for p in k.processes.values() {
            if p.pid == crate::kernel::INIT_PID || p.pid == st.me.pid {
                continue;
            }
            for h in p.handles.values() {
                if let crate::kernel::Object::Budget(b) = h.object {
                    if k.budgets.get(&b).is_some_and(|x| x.class == crate::spec::Class::System) {
                        return Err(format!("P12: process {} holds system-class budget {b}", p.pid));
                    }
                }
            }
        }
        for (pid, o) in &st.procs {
            let (Some(p), Some(b)) = (k.processes.get(pid), st.budget_of(o)) else { continue };
            for h in p.handles.values().filter(|h| matches!(h.object, crate::kernel::Object::Endpoint(_))) {
                let stamp = k.budgets.get(&h.stamp);
                let scope =
                    stamp.is_some_and(|x| x.pages_limit == 0 && x.processes_limit == 0 && x.weight == 0);
                if !scope || !k.is_descendant_or_self(h.stamp, b) {
                    return Err(format!(
                        "P12: {o:?}'s connection is stamped {}, not a scope in its budget",
                        h.stamp
                    ));
                }
                self.reached.insert("P12 a session's connection");
            }
        }
        // P17: one live session of a context's name per domain.
        for (d, s) in inspect::domains(&st.store) {
            let mut names: BTreeSet<&str> = BTreeSet::new();
            for x in s.sessions.values().filter(|x| x.state != redoubt_steward::gen::session::State::Ending) {
                let Some(name) = x.context.as_deref() else { continue };
                if !names.insert(name) {
                    return Err(format!("P17: {d:?} holds two live sessions of context {name:?}"));
                }
                if !name.is_empty() {
                    self.reached.insert("P17 a named context");
                }
            }
        }
        // P5: the cap per domain, and no request of an ended session or agent.
        for (d, s) in inspect::domains(&st.store) {
            if s.requests.len() > PENDING_CAP {
                return Err(format!("P5: {d:?} has {} pending requests", s.requests.len()));
            }
            if s.requests.len() == PENDING_CAP {
                self.reached.insert("P5 a domain at the cap");
            }
            for r in s.requests.values() {
                let live = match r.by.kind {
                    Kind::Session => s.sessions.contains_key(&r.by.id),
                    _ => s.leases.contains_key(&r.by.id),
                };
                if !live {
                    return Err(format!(
                        "P5: request {} of ended {:?} {} is still pending",
                        r.id, r.by.kind, r.by.id
                    ));
                }
            }
        }
        let mut copied: Vec<&Vec<u8>> = Vec::new();
        for a in &st.audit[audit_from..] {
            let labels = a.domain().labels();
            let instance = match a.record() {
                Record::Login { .. } => Some("P2 a login"),
                Record::Approved { .. } => Some("P3 an approval"),
                Record::AgentStarted { .. } => Some("P9 an agent starts"),
                Record::LeaseEnded { .. } => Some("P9 a lease ends"),
                Record::Declassified { .. } => Some("P6 a declassification"),
                Record::Pushed { .. } => Some("P6 a push"),
                _ => None,
            };
            if let Some(r) = instance {
                self.reached.insert(r);
            }
            match a.record() {
                Record::Login { principal, key, .. }
                    if !fixed.principals[*principal].login_keys.contains(key) || fixed.keyd.contains(key) =>
                {
                    return Err(format!("P2: login to principal {principal} with key {key}"));
                }
                Record::Approved { request, principal, key, .. } => {
                    let p = &fixed.principals[*principal];
                    if !self.ghost.contains_key(request) {
                        return Err(format!("P3: approved request {request} was never submitted"));
                    }
                    if !p.approval_keys.contains(key)
                        || p.login_keys.contains(key)
                        || fixed.keyd.contains(key)
                    {
                        return Err(format!(
                            "P3: request {request} approved with key {key}, not an approval key"
                        ));
                    }
                }
                Record::AgentStarted { sponsor, deadline, .. } => {
                    if !fixed.principals[*sponsor].owned.includes(labels) {
                        return Err(format!(
                            "P3: an agent labelled {labels:?} started for a principal owning fewer"
                        ));
                    }
                    if *deadline > k.now.saturating_add(MAX_LEASE) {
                        return Err(format!(
                            "P9: an agent's lease ends at {deadline}, over MAX_LEASE from {}",
                            k.now
                        ));
                    }
                }
                Record::Declassified { request, bytes, reader } => {
                    let g = self
                        .ghost
                        .get(request)
                        .ok_or(format!("P6: declassified request {request} was never submitted"))?;
                    let Content::Declassify { labels: item, .. } = &g.content else {
                        return Err(format!(
                            "P6: request {request} declassified, but it asked for {:?}",
                            g.content
                        ));
                    };
                    let item = Labels::new(item).unwrap_or_default();
                    if *g.by.domain.labels() != item {
                        return Err(format!(
                            "P6: item of {item:?} declassified from a session labelled {:?}",
                            g.by.domain.labels()
                        ));
                    }
                    if g.snapshot.as_ref() != Some(bytes) {
                        return Err(format!(
                            "P6: declassified {bytes:?}, the snapshot at submission was {:?}",
                            g.snapshot
                        ));
                    }
                    if bytes.len() > DECLASSIFY_MAX || !printable(bytes) {
                        return Err(format!(
                            "P6: declassified {} bytes, not printable text within DECLASSIFY_MAX",
                            bytes.len()
                        ));
                    }
                    // The kernel's own record of the reader budget.
                    let read = k.ghost.labels_at_creation.get(reader);
                    if read.map(|l| l.as_slice()) != Some(item.as_slice()) {
                        return Err(format!(
                            "P6: item of {item:?} read through budget {reader} with labels {read:?}"
                        ));
                    }
                    copied.push(bytes);
                }
                Record::Pushed { request, bytes, writer, .. } => {
                    let g = self
                        .ghost
                        .get(request)
                        .ok_or(format!("P6: pushed request {request} was never submitted"))?;
                    if g.by.labelled() {
                        return Err(format!(
                            "P6: a push submitted from a session labelled {:?}",
                            g.by.domain.labels()
                        ));
                    }
                    if g.snapshot.as_ref() != Some(bytes) {
                        return Err(format!(
                            "P6: pushed {bytes:?}, the snapshot at submission was {:?}",
                            g.snapshot
                        ));
                    }
                    let wrote = k.ghost.labels_at_creation.get(writer);
                    if wrote.map(|l| l.as_slice()) != Some(labels.as_slice()) {
                        return Err(format!(
                            "P6: item of {labels:?} pushed through budget {writer} with labels {wrote:?}"
                        ));
                    }
                }
                _ => {}
            }
        }
        // P11, and P6's copy out: what reached the unlabelled volume is the snapshot.
        for w in &st.writes[writes_from..] {
            match &w.through {
                Some(t) if *t == w.labels => {
                    self.reached.insert("P11 a write");
                }
                None if w.by.kind == Kind::Crossing && w.labels.is_empty() => {
                    self.reached.insert("P6 a copy out");
                    if !copied.contains(&&w.bytes) {
                        return Err(format!(
                            "P6: the copy out wrote {:?}, which is no snapshot approved now",
                            w.bytes
                        ));
                    }
                }
                t => {
                    return Err(format!(
                        "P11: item {} of {:?} written by {:?} through {t:?}",
                        w.item, w.labels, w.by
                    ));
                }
            }
        }
        Ok(())
    }

    /// P4, and P3's frozen hash, on the screens this step showed; each binds its request to its
    /// channel, as the family records it.
    fn screens(&mut self, outputs_from: usize) -> Result<(), String> {
        let fixed = inspect::fixed(&self.st.store);
        for o in &self.st.outputs[outputs_from..] {
            let Output::Screen { channel, screen: r } = o else { continue };
            let ch = self
                .channels
                .iter()
                .find(|c| c.id == *channel)
                .ok_or(format!("P4: a screen on unknown channel {channel}"))?;
            let g =
                self.ghost.get(&r.id).ok_or(format!("P4: a screen of request {} never submitted", r.id))?;
            let owned = &fixed.principals[ch.principal].owned;
            if g.by.principal != ch.principal
                || !owned.includes(g.by.domain.labels())
                || r.labels != *g.by.domain.labels()
            {
                return Err(format!(
                    "P4: principal {} sees request {} labelled {:?}",
                    ch.principal, r.id, r.labels
                ));
            }
            self.reached.insert(if g.by.labelled() {
                "P4 a labelled request's screen"
            } else {
                "P4 a screen"
            });
            if r.text.chars().any(|c| !(' '..='~').contains(&c)) {
                return Err(format!("P4: rendered request {} is not printable ASCII: {:?}", r.id, r.text));
            }
            // A labelled request shows only text the steward generates.
            let note = match &g.content {
                Content::Note { what } => what.clone(),
                _ => String::new(),
            };
            for free in [&g.reason, &note] {
                let shown = sanitize(free, FIELD_CAP);
                if g.by.labelled() && !shown.is_empty() && r.text.contains(&format!("\"{shown}\"")) {
                    return Err(format!("P4: labelled request {} shows its free text: {:?}", r.id, r.text));
                }
            }
            let first = self.shown.get(&r.id).map_or(r.hash, |s| s.0);
            if first != r.hash {
                return Err(format!("P3: request {}'s hash changed while it was frozen", r.id));
            }
            self.shown.insert(r.id, (first, *channel));
        }
        Ok(())
    }
}

/// P1-P9, P11-P14: a random sequence of policy operations.
pub fn steward_policy(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    policy_sequence(seed, mutation, false, crate::mutation::policy(mutation)).map(|_| ())
}

/// The coverage instrument's run of `steward_policy`'s sequence for `seed`, unmutated, with the
/// core deciding by `policy`: the properties' instances its checks met.
pub fn steward_policy_reach(seed: u64, policy: Policy) -> Result<BTreeSet<&'static str>, Failure> {
    policy_sequence(seed, None, false, policy).map(|run| run.reached)
}

/// The events `steward_policy`'s sequence for `seed` gives the core: a trace for the Elixir
/// reference (servers/steward.md, "Two embedders and a reference").
pub fn steward_policy_events(seed: u64) -> Result<Vec<Event>, Failure> {
    policy_sequence(seed, None, true, Policy::SHIPPED)
        .map(|mut run| run.st.recorded.take().unwrap_or_default())
}

fn policy_sequence(
    seed: u64,
    mutation: Option<Mutation>,
    record: bool,
    policy: Policy,
) -> Result<Run, Failure> {
    let mut rng = Rng::new(seed);
    let mut run = Run::with_policy(mutation, rng.next_u64(), policy);
    run.st.recorded = record.then(Vec::new);
    for i in 0..rng.range(20, 160) {
        let op = random_op(&run, &mut rng);
        run.apply(&op).map_err(|message| Failure {
            family: "steward_policy",
            seed,
            message: format!("{message} (at op {i}: {op:?})"),
            ops: Vec::new(),
        })?;
    }
    Ok(run)
}

/// P10 (servers/steward.md R37): one sequence runs twice, the second time without the work of the
/// vault sessions: their item writes, submissions and calls to the shared server, the owner's
/// approvals and denials of their requests, and whatever names a session or agent such work
/// started. Everything an unlabelled session observes must be the same: every result it gets,
/// the ids of its requests, the order the server takes its calls in, the audit records an
/// unlabelled reader may read, and the usage of every principal's top budget and unlabelled
/// sub-budget and of `users`.
///
/// Since the caps are per domain, the observers include the vault owner's own unlabelled
/// sessions. Ending a vault session, and a crash an unlabelled session's call causes, run in
/// both. Left out of the sequence: a crash at an instant, and one a vault's call causes, whose
/// blame and outcomes are service-slot timing (servers/steward.md R37, "Residual risks"). The
/// owner's approval screen is not an observer.
pub fn steward_noninterference(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    noninterference_runs(seed, mutation, false, crate::mutation::policy(mutation)).map(|_| ())
}

/// The coverage instrument's run of `steward_noninterference`'s two runs for `seed`, unmutated,
/// with the core deciding by `policy`: the properties' instances their checks met.
pub fn steward_noninterference_reach(seed: u64, policy: Policy) -> Result<BTreeSet<&'static str>, Failure> {
    noninterference_runs(seed, None, false, policy).map(|(with, without)| &with.reached | &without.reached)
}

/// The events `steward_noninterference`'s two runs for `seed` give the core, with the vault's
/// work and without it: traces for the Elixir reference.
pub fn steward_noninterference_events(seed: u64) -> Result<[Vec<Event>; 2], Failure> {
    let (mut with, mut without) = noninterference_runs(seed, None, true, Policy::SHIPPED)?;
    Ok([with.st.recorded.take().unwrap_or_default(), without.st.recorded.take().unwrap_or_default()])
}

fn noninterference_runs(
    seed: u64,
    mutation: Option<Mutation>,
    record: bool,
    policy: Policy,
) -> Result<(Run, Run), Failure> {
    let mut rng = Rng::new(seed);
    let secret = rng.next_u64();
    let mut left = rng.range(20, 100);
    let mut next = |first: &Run| {
        left = left.checked_sub(1)?;
        Some(random_op(first, &mut rng))
    };
    paired_runs(seed, secret, mutation, record, policy, &mut next)
}

/// P10 on a scripted sequence (a directed scenario, model/tests/common/contracts.rs): `next`
/// gives each op from the state of the run with the vault's work so far, until it gives none.
pub fn steward_noninterference_script(
    mutation: Option<Mutation>,
    next: &mut dyn FnMut(&Run) -> Option<PolicyOp>,
) -> Result<(), Failure> {
    paired_runs(0, 0, mutation, false, crate::mutation::policy(mutation), next).map(|_| ())
}

/// The two runs of P10: the sequence `next` gives, with the vault's work and without it.
fn paired_runs(
    seed: u64,
    secret: u64,
    mutation: Option<Mutation>,
    record: bool,
    policy: Policy,
    next: &mut dyn FnMut(&Run) -> Option<PolicyOp>,
) -> Result<(Run, Run), Failure> {
    let fail =
        |message: String| Failure { family: "steward_noninterference", seed, message, ops: Vec::new() };
    // Build the sequence on a first run, recording which ops are vault work.
    let mut first = Run::with_policy(mutation, secret, policy);
    let mut ops: Vec<(PolicyOp, bool)> = Vec::new();
    let mut vault_sessions: BTreeSet<u64> = BTreeSet::new();
    // Sessions and agents vault work started: they exist only with the vault's work, so an op
    // that names one is vault work too.
    let mut vault_made: BTreeSet<u64> = BTreeSet::new();
    let mut owners: BTreeSet<usize> = BTreeSet::new();
    while let Some(op) = next(&first) {
        let vault = match &op {
            PolicyOp::WriteItem { session, .. }
            | PolicyOp::Submit { session, .. }
            | PolicyOp::Work { session } => vault_sessions.contains(session),
            PolicyOp::Approve { request, .. } | PolicyOp::Deny { request, .. } => {
                vault_sessions.contains(&request.session)
            }
            PolicyOp::EndSession { session } | PolicyOp::StartAgent { session, .. } => {
                vault_made.contains(session)
            }
            PolicyOp::EndLease { by, lease } => vault_made.contains(by) || vault_made.contains(lease),
            // A crash at an instant blames whichever call is in service, which the vault's queued
            // calls decide, and a crash the vault's call causes moves when the server takes the
            // calls before it: service-slot timing (servers/steward.md R37, "Residual risks").
            PolicyOp::CrashServing { session } if vault_sessions.contains(session) => continue,
            PolicyOp::Hold | PolicyOp::Crash => continue,
            _ => false,
        };
        let before: BTreeSet<u64> = first.st.callers().iter().map(|c| c.id).collect();
        first.apply(&op).map_err(fail)?;
        for c in first.st.callers().into_iter().filter(|c| !before.contains(&c.id)) {
            if vault {
                vault_made.insert(c.id);
            }
            if c.labelled() {
                vault_sessions.insert(c.id);
                owners.insert(c.principal);
            }
        }
        ops.push((op, vault));
    }
    let mut with = Run::with_policy(mutation, secret, policy);
    let mut without = Run::with_policy(mutation, secret, policy);
    with.st.recorded = record.then(Vec::new);
    without.st.recorded = record.then(Vec::new);
    // A vault session's id follows its domain's own history, which the vault's work is part of:
    // the session an op names is found in the run without that work as the one started at the
    // same op. Results are never renamed.
    let mut renamed: BTreeMap<u64, u64> = BTreeMap::new();
    // How much of each run's take log has been read, and the unlabelled takes read from it.
    let mut seen = (0, 0);
    let mut order: (Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new());
    // Whether vault work has been left out of the run without it yet.
    let mut left_out = false;
    let unlabelled = |r: &Run, id: &u64| r.st.caller(*id).is_some_and(|c| !c.labelled());
    for (i, (op, vault)) in ops.iter().enumerate() {
        let observed = match op {
            PolicyOp::Pending { channel } => !owners.contains(&with.channel(*channel).principal),
            PolicyOp::EndSession { session }
            | PolicyOp::StartAgent { session, .. }
            | PolicyOp::WriteItem { session, .. }
            | PolicyOp::Submit { session, .. }
            | PolicyOp::Work { session } => unlabelled(&with, session),
            PolicyOp::Login { labels, .. } => labels.is_empty(),
            PolicyOp::EndLease { by, .. } => unlabelled(&with, by),
            PolicyOp::Open { .. }
            | PolicyOp::Approve { .. }
            | PolicyOp::Deny { .. }
            | PolicyOp::Usage { .. } => true,
            _ => false,
        };
        let ids = |r: &Run| r.st.callers().iter().map(|c| c.id).collect::<BTreeSet<u64>>();
        let (before_with, before_without) = (ids(&with), ids(&without));
        let a = with.apply(op).map_err(fail)?;
        if *vault {
            left_out = true;
            continue;
        }
        let b = without.apply(&rename(op, &renamed)).map_err(fail)?;
        let mut started: Vec<Caller> =
            without.st.callers().into_iter().filter(|c| !before_without.contains(&c.id)).collect();
        for c in with.st.callers().into_iter().filter(|c| !before_with.contains(&c.id)) {
            let same = |t: &Caller| t.principal == c.principal && t.domain == c.domain && t.kind == c.kind;
            if let Some(at) = started.iter().position(same) {
                renamed.insert(c.id, started.remove(at).id);
            }
        }
        if observed && left_out {
            with.reached.insert("P10 an observation after vault work left out");
        }
        if observed && a != b {
            return Err(fail(format!(
                "P10: op {i} ({op:?}) showed {a:?} with the vault's work and {b:?} without"
            )));
        }
        // Ids of unlabelled sessions' requests.
        for (key, id) in &with.submitted {
            if unlabelled(&with, &key.0) && without.submitted.get(key) != Some(id) {
                return Err(fail(format!(
                    "P10: session {}'s request {} got a different id without the vault's work",
                    key.0, key.1
                )));
            }
        }
        // The unlabelled sessions' calls the server took, and their order (R2: one label set's turns
        // do not depend on another's). Each op's takes are added to what was already compared.
        for (r, seen, order) in [(&with, &mut seen.0, &mut order.0), (&without, &mut seen.1, &mut order.1)] {
            order.extend(r.st.taken[*seen..].iter().filter(|(labels, _)| labels.is_empty()).map(|(_, w)| *w));
            *seen = r.st.taken.len();
        }
        if order.0 != order.1 {
            return Err(fail(format!("P10: the server took unlabelled calls in another order (op {i})")));
        }
        // What an unlabelled reader may read of the audit file.
        let (x, y) = (with.st.audit_view(&[]), without.st.audit_view(&[]));
        if x != y {
            let at = x.iter().zip(&y).position(|(a, b)| a != b).unwrap_or(x.len().min(y.len()));
            return Err(fail(format!(
                "P10: the unlabelled audit view depends on the vault's work (op {i} {op:?}: {:?} with, {:?} without)",
                x.get(at),
                y.get(at)
            )));
        }
        // The steward's slot 1 is `users`; each principal's top budget and unlabelled sub-budget.
        let shared: Vec<u64> = core::iter::once(1)
            .chain(with.st.tops.values().map(|t| t.1))
            .chain(with.st.subs.iter().filter(|(d, _)| d.labels().is_empty()).map(|(_, s)| s.1))
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
    Ok((with, without))
}

/// `op` with the sessions it names as the run without the vault's work knows them
/// (`steward_noninterference`).
fn rename(op: &PolicyOp, to: &BTreeMap<u64, u64>) -> PolicyOp {
    let s = |x: u64| *to.get(&x).unwrap_or(&x);
    let r = |x: &ReqRef| ReqRef { session: s(x.session), nth: x.nth };
    let mut op = op.clone();
    match &mut op {
        PolicyOp::EndSession { session }
        | PolicyOp::StartAgent { session, .. }
        | PolicyOp::WriteItem { session, .. }
        | PolicyOp::Submit { session, .. }
        | PolicyOp::Work { session }
        | PolicyOp::CrashServing { session } => *session = s(*session),
        PolicyOp::Approve { request, hash, .. } => {
            *request = r(request);
            if let HashRef::Of(x) = hash {
                *x = r(x);
            }
        }
        PolicyOp::Deny { request, .. } => *request = r(request),
        PolicyOp::EndLease { by, lease } => {
            *by = s(*by);
            *lease = s(*lease);
        }
        _ => {}
    }
    op
}

/// P15. Independent ancestry oracle for the self-minted share (servers/serving.md R26: a chain of
/// self-mints spends one share). The model stores a root at grant time; this checker instead walks
/// immutable parent edges and reconstructs connected shares after every operation, including after
/// disconnect. `break_lineage` deliberately assigns self-minted descendants new roots to establish
/// that the property can fail.
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

/// P16. A specification-side observable read oracle. Confined read-down must fail even when the
/// lower volume exists and ordinary subset-based `check` would allow the read. Accepting a
/// supplied success here is the deliberate rule-break control in `policy_current`.
pub fn confined_read_observation(
    confined: bool,
    caller_labels: &[u64],
    object_labels: &[u64],
    result: &Result<Vec<u8>, Denied>,
) -> Result<(), String> {
    if confined && !caller_labels.is_empty() && object_labels.is_empty() && result != &Err(Denied::ReadDown) {
        return Err(String::from("P16: a confined labelled caller read a shared unlabelled volume"));
    }
    Ok(())
}
