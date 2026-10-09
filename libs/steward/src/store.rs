//! The store: partitioned by domain (servers/steward.md, "Domains").
//!
//! Outside every domain is what the manifest fixes (`Fixed`), the routing index (where each
//! session and lease is: written when one starts or ends, read to route an event and to address a
//! notice) and the approval channels, which belong to a principal across its label sets. A
//! handler sees one domain. Only `edges` can borrow two at once.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::domain::{Domain, Labels};
use crate::edges::TwoDomains;
use crate::effect::{Kind, Object, ReplyTo};
use crate::event::Content;
use crate::gen::{self, Policy};
use crate::manifest::Fixed;

/// A login session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: u64,
    pub state: gen::session::State,
    pub principal: usize,
    /// The key it logged in with.
    pub key: u64,
    /// The context it is (servers/steward.md, "Contexts"): empty for the principal's default one;
    /// `None` for the console's session, which is no context.
    pub context: Option<String>,
    /// Its badge on the steward's endpoint, which routes its events.
    pub badge: u64,
    /// Numbered per domain (`session-3`), from 1 when it starts running; 0 before.
    pub number: u64,
    /// The call a batch's outcome answers.
    pub reply: ReplyTo,
    /// A context's current attachment: the id its channel is named by, 0 while it is detached
    /// (servers/steward.md, "Contexts"). A login's first is the session's own id.
    pub attachment: u64,
    /// The client address of the current attachment, as `sshd` gave it, checked.
    pub from: String,
}

/// An agent on a lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    pub id: u64,
    pub state: gen::lease::State,
    /// The sponsor.
    pub principal: usize,
    pub badge: u64,
    /// Numbered per domain (`agent-7`).
    pub number: u64,
    /// How long it was asked for, and when it ends.
    pub lease: u64,
    pub deadline: u64,
    /// The agent it runs inside, for a sub-agent: same domain.
    pub parent: Option<u64>,
    /// Started by an approved request: no caller waits for it.
    pub granted: bool,
    pub reply: ReplyTo,
}

/// Who submitted a request: a session or a lease of the same domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requester {
    pub kind: Kind,
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub state: gen::request::State,
    pub by: Requester,
    /// The requester's principal: the one who may approve.
    pub principal: usize,
    pub content: Content,
    pub reason: String,
    /// The item as it was at submission (a declassification or a push).
    pub snapshot: Option<Vec<u8>>,
    /// The kernel's id of the budget the snapshot was read through, 0 for none.
    pub reader: u64,
    /// The binding hash, set when it is frozen.
    pub hash: [u8; 32],
    /// The domain its records are read under: the target's for a labelled agent or a push.
    pub audit: Domain,
    /// The channel that rendered it last.
    pub channel: Option<u64>,
    pub reply: ReplyTo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossingKind {
    /// A declassification's read, at submission.
    Read,
    /// A declassification's copy out, on approval: the steward's own write, no budget.
    CopyOut,
    /// A push's write, on approval.
    Write,
}

/// One item moving between a labelled domain (this one) and the unlabelled one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crossing {
    pub id: u64,
    pub state: gen::crossing::State,
    pub kind: CrossingKind,
    /// The request it serves, and that request's domain.
    pub request: Object,
    /// The item: read from or written to this domain's volume, or (a copy out) the unlabelled
    /// one.
    pub item: u64,
    /// The snapshot it writes, for a copy out or a write.
    pub bytes: Vec<u8>,
    /// For a push, the unlabelled item it came from; for a copy out, the reader's kernel id.
    pub source: u64,
    /// Whether it carved a budget to go through.
    pub through: bool,
}

/// A domain's crash blame (R40).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blame {
    pub state: gen::blame::State,
    /// The latest blames' times, at most `BLAME_COUNT`.
    pub times: Vec<u64>,
    /// When its lockout ends.
    pub until: u64,
}

/// An `ssh approve@box` connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub id: u64,
    pub state: gen::approval_channel::State,
    pub principal: usize,
    pub key: u64,
}

/// One domain's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainState {
    pub sessions: BTreeMap<u64, Session>,
    pub leases: BTreeMap<u64, Lease>,
    pub requests: BTreeMap<u64, Request>,
    pub crossings: BTreeMap<u64, Crossing>,
    pub blame: Blame,
    /// Sessions and agents started so far: they number and name them, per domain (R37).
    pub sessions_started: u64,
    pub agents_started: u64,
}

impl DomainState {
    /// An empty domain's state: what boot starts each domain with, and the scratch state the
    /// approval channel machine runs against.
    pub(crate) fn scratch() -> DomainState { DomainState::new() }

    fn new() -> DomainState {
        DomainState {
            sessions: BTreeMap::new(),
            leases: BTreeMap::new(),
            requests: BTreeMap::new(),
            crossings: BTreeMap::new(),
            blame: Blame { state: gen::blame::State::Open, times: Vec::new(), until: 0 },
            sessions_started: 0,
            agents_started: 0,
        }
    }

    /// Whether an object of this kind and id is here.
    pub fn has(&self, kind: Kind, id: u64) -> bool {
        match kind {
            Kind::Session => self.sessions.contains_key(&id),
            Kind::Lease => self.leases.contains_key(&id),
            Kind::Request => self.requests.contains_key(&id),
            Kind::Crossing => self.crossings.contains_key(&id),
            Kind::Blame => id == 0,
            Kind::Channel => false,
        }
    }
}

/// Where a badge routes: a running session or lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub domain: Domain,
    pub kind: Kind,
    pub id: u64,
}

/// The routing index and the approval channels: outside every domain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    pub routes: BTreeMap<u64, Route>,
    /// A session's or lease's id, to its domain.
    pub ids: BTreeMap<u64, (Domain, Kind)>,
    pub channels: BTreeMap<u64, Channel>,
    /// A context's current attachment id, to its session's domain and id.
    pub attachments: BTreeMap<u64, (Domain, u64)>,
}

impl Index {
    /// Whether `id` names nothing yet: no object of any kind, in any domain, and no badge.
    pub(crate) fn fresh(&self, id: u64) -> bool {
        id != 0
            && !self.ids.contains_key(&id)
            && !self.routes.contains_key(&id)
            && !self.channels.contains_key(&id)
            && !self.attachments.contains_key(&id)
    }
}

/// The parts of the store one transition borrows: the fixed part, the index, and one domain.
pub(crate) struct Parts<'a> {
    pub fixed: &'a Fixed,
    pub index: &'a mut Index,
    pub used: &'a BTreeMap<u64, Domain>,
    pub domain: &'a Domain,
    pub state: &'a mut DomainState,
}

/// Everything the steward decides from.
#[derive(Clone)]
pub struct Store {
    pub(crate) fixed: Fixed,
    pub(crate) policy: gen::Policy,
    pub(crate) index: Index,
    /// Domains in the manifest's order; `at` finds one. Private: a handler gets one domain
    /// (`parts`), and only `edges` two (`pair`).
    domains: Vec<(Domain, DomainState)>,
    at: BTreeMap<Domain, usize>,
    /// Request and crossing ids in use, which `Index` does not hold: so no id is given twice.
    pub(crate) used: BTreeMap<u64, Domain>,
    /// Set when the steward exited (an `unreachable` row): it decides nothing more.
    pub(crate) exited: bool,
}

impl Store {
    /// The store for `fixed`: one domain per principal's label set, each with its blame open.
    pub(crate) fn new(fixed: Fixed, policy: gen::Policy) -> Store {
        let mut domains = Vec::new();
        let mut at = BTreeMap::new();
        for p in &fixed.principals {
            for d in &p.domains {
                at.insert(d.clone(), domains.len());
                domains.push((d.clone(), DomainState::new()));
            }
        }
        Store { fixed, policy, index: Index::default(), domains, at, used: BTreeMap::new(), exited: false }
    }

    pub(crate) fn domain_mut(&mut self, d: &Domain) -> Option<&mut DomainState> {
        let i = *self.at.get(d)?;
        Some(&mut self.domains[i].1)
    }

    pub fn domain(&self, d: &Domain) -> Option<&DomainState> { self.at.get(d).map(|i| &self.domains[*i].1) }

    /// The domains of `account`.
    pub(crate) fn of_account(&self, account: u64) -> impl Iterator<Item = &Domain> {
        self.domains.iter().map(|(d, _)| d).filter(move |d| d.account().get() == account)
    }

    /// A closed channel's renders: each request it rendered last needs a render again, so a
    /// channel opened later under its id answers none of them (R38's binding).
    pub(crate) fn unbind(&mut self, channel: u64) {
        for (_, s) in &mut self.domains {
            for r in s.requests.values_mut().filter(|r| r.channel == Some(channel)) {
                r.channel = None;
            }
        }
    }

    /// Every domain and its state, to read.
    pub(crate) fn all(&self) -> impl Iterator<Item = (&Domain, &DomainState)> {
        self.domains.iter().map(|(d, s)| (d, s))
    }

    /// What one transition in `d` borrows: the policy, and the store's parts with that one
    /// domain.
    pub(crate) fn parts(&mut self, d: &Domain) -> Option<(&Policy, Parts<'_>)> {
        let i = *self.at.get(d)?;
        let (domain, state) = &mut self.domains[i];
        let parts = Parts { fixed: &self.fixed, index: &mut self.index, used: &self.used, domain, state };
        Some((&self.policy, parts))
    }

    /// The same with a second, different domain to read. Only the module that holds the three
    /// cross-domain edges can make the key it takes (servers/steward.md, "Domains").
    pub(crate) fn pair(
        &mut self,
        _: &TwoDomains,
        a: &Domain,
        b: &Domain,
    ) -> Option<(&Policy, Parts<'_>, &DomainState)> {
        let (i, j) = (*self.at.get(a)?, *self.at.get(b)?);
        let ((domain, state), other) = if i < j {
            let (lo, hi) = self.domains.split_at_mut(j);
            (&mut lo[i], &hi[0].1)
        } else if j < i {
            let (lo, hi) = self.domains.split_at_mut(i);
            (&mut hi[0], &lo[j].1)
        } else {
            return None;
        };
        let parts = Parts { fixed: &self.fixed, index: &mut self.index, used: &self.used, domain, state };
        Some((&self.policy, parts, other))
    }

    /// The domain of a label set of `account`, if the manifest names it.
    pub(crate) fn find(&self, account: u64, labels: &[u64]) -> Option<Domain> {
        let labels = Labels::new(labels)?;
        self.of_account(account).find(|d| *d.labels() == labels).cloned()
    }
}
